//! `os.spawn` `os.wait` `os.poll` `os.kill`: 子プロセスの常駐連携（design.md 5.42）。
//! 標準入出力は、ストリーム（5.41）として渡す。標準ライブラリだけを使う

use std::io::{self, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::net::closed;
use super::stream::stream_value;
use super::{error_value, expect_args, new_map, num_arg, str_arg, type_error};
use crate::runtime::handle::{Handle, HandleKind, ProcessHandle, StreamSink};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

/// 書くたびに相手へ送る（パイプで対話するとき、`flush` を忘れて止まらないように）
struct FlushEach<W: Write>(W);

impl<W: Write> Write for FlushEach<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.0.write(buf)?;
        self.0.flush()?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Pipe,
    Null,
    Inherit,
}

impl Mode {
    fn stdio(self) -> Stdio {
        match self {
            Mode::Pipe => Stdio::piped(),
            Mode::Null => Stdio::null(),
            Mode::Inherit => Stdio::inherit(),
        }
    }
}

struct Options {
    cwd: Option<String>,
    env: Vec<(String, String)>,
    stdin: Mode,
    stdout: Mode,
    stderr: Mode,
}

fn options(value: &Value) -> Result<Options, String> {
    let Value::Map(map) = value else {
        return Err(type_error("os.spawn", 2, "マップ", value));
    };
    let map = map.borrow();
    const KNOWN: [&str; 5] = ["cwd", "env", "stdin", "stdout", "stderr"];
    for (key, _) in map.iter() {
        if !KNOWN.contains(&&**key) {
            return Err(format!(
                "os.spawn に知らないオプション `{key}` があります（使えるのは {}）",
                KNOWN.join("、")
            ));
        }
    }
    let mode = |key: &str, default: Mode| -> Result<Mode, String> {
        match map.get(key) {
            None => Ok(default),
            Some(Value::Str(s)) => match &**s {
                "pipe" => Ok(Mode::Pipe),
                "null" => Ok(Mode::Null),
                "inherit" => Ok(Mode::Inherit),
                other => Err(format!(
                    "os.spawn の `{key}` は \"pipe\" \"null\" \"inherit\" のどれかです（{other:?}）"
                )),
            },
            Some(other) => Err(format!(
                "os.spawn の `{key}` は文字列です（{}が渡されました）",
                other.type_name()
            )),
        }
    };
    let cwd = match map.get("cwd") {
        None => None,
        Some(Value::Str(s)) => Some(s.to_string()),
        Some(other) => {
            return Err(format!(
                "os.spawn の `cwd` は文字列です（{}が渡されました）",
                other.type_name()
            ));
        }
    };
    let mut env = Vec::new();
    match map.get("env") {
        None => {}
        Some(Value::Map(vars)) => {
            for (key, value) in vars.borrow().iter() {
                let Value::Str(value) = value else {
                    return Err(format!(
                        "os.spawn の `env` の `{key}` は文字列です（{}が渡されました）",
                        value.type_name()
                    ));
                };
                if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
                    return Err(format!(
                        "os.spawn の `env` の `{key}` は環境変数にできません"
                    ));
                }
                env.push((key.to_string(), value.to_string()));
            }
        }
        Some(other) => {
            return Err(format!(
                "os.spawn の `env` はマップです（{}が渡されました）",
                other.type_name()
            ));
        }
    }
    Ok(Options {
        cwd,
        env,
        stdin: mode("stdin", Mode::Pipe)?,
        stdout: mode("stdout", Mode::Pipe)?,
        stderr: mode("stderr", Mode::Inherit)?,
    })
}

fn process_pid(pid: u32) -> Value {
    Value::Num(f64::from(pid))
}

/// 起動して、`pid`・プロセス・標準入出力のストリームのマップを返す
pub fn spawn(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "os.spawn の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let command = str_arg("os.spawn", args, 0)?;
    let Value::Array(list) = &args[1] else {
        return Err(type_error("os.spawn", 1, "配列", &args[1]));
    };
    let mut arguments = Vec::new();
    for (i, item) in list.borrow().iter().enumerate() {
        match item {
            Value::Str(s) if !s.contains('\0') => arguments.push(s.to_string()),
            Value::Str(_) => return Err(format!("os.spawn の引数 {i} 番目に NUL は使えません")),
            other => {
                return Err(format!(
                    "os.spawn の引数の配列の {i} 番目は文字列です（{}が渡されました）",
                    other.type_name()
                ));
            }
        }
    }
    let options = match args.get(2) {
        Some(value) => options(value)?,
        None => Options {
            cwd: None,
            env: Vec::new(),
            stdin: Mode::Pipe,
            stdout: Mode::Pipe,
            stderr: Mode::Inherit,
        },
    };
    if command.is_empty() || command.contains('\0') {
        return Err("os.spawn のコマンド名が正しくありません".into());
    }
    // 子プロセスの出力が、こちらの出力と前後しないように
    interp.out.flush().map_err(|e| e.to_string())?;

    if let Some(dir) = &options.cwd {
        match std::fs::metadata(dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Ok(error_value(
                    "io",
                    format!("`{dir}` はディレクトリではありません"),
                ));
            }
            Err(e) => {
                let (kind, reason) = super::fs::classify(&e);
                return Ok(error_value(
                    kind,
                    format!("`{dir}` をカレントディレクトリにできません: {reason}"),
                ));
            }
        }
    }
    let mut cmd = Command::new(command);
    cmd.args(&arguments)
        .stdin(options.stdin.stdio())
        .stdout(options.stdout.stdio())
        .stderr(options.stderr.stdio());
    if let Some(dir) = &options.cwd {
        cmd.current_dir(dir);
    }
    for (key, value) in &options.env {
        cmd.env(key, value);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let (kind, reason) = match e.kind() {
                io::ErrorKind::NotFound => (
                    "not_found",
                    "コマンドかカレントディレクトリが見つかりません".to_string(),
                ),
                _ => super::fs::classify(&e),
            };
            return Ok(error_value(
                kind,
                format!("`{command}` を実行できません: {reason}"),
            ));
        }
    };
    let pid = child.id();
    let stdin = child.stdin.take().map(|pipe| {
        stream_value(
            None,
            Some(StreamSink::Writer(Box::new(FlushEach(pipe)))),
            format!("プロセス {pid} 標準入力"),
        )
    });
    let stdout = child.stdout.take().map(|pipe| {
        stream_value(
            Some(Box::new(BufReader::new(pipe))),
            None,
            format!("プロセス {pid} 標準出力"),
        )
    });
    let stderr = child.stderr.take().map(|pipe| {
        stream_value(
            Some(Box::new(BufReader::new(pipe))),
            None,
            format!("プロセス {pid} 標準エラー出力"),
        )
    });
    let process = Value::Handle(Handle::new(HandleKind::Process(ProcessHandle {
        child,
        pid,
        result: None,
    })));
    let mut map = Map::default();
    map.insert("pid".into(), process_pid(pid));
    map.insert("process".into(), process);
    map.insert("stdin".into(), stdin.unwrap_or(Value::Nil));
    map.insert("stdout".into(), stdout.unwrap_or(Value::Nil));
    map.insert("stderr".into(), stderr.unwrap_or(Value::Nil));
    Ok(new_map(map))
}

/// プロセスのハンドルに `f` を適用する。閉じていればエラー値、プロセスでなければ実行時エラー
fn with_process(
    name: &str,
    args: &[Value],
    f: impl FnOnce(&mut ProcessHandle) -> Result<Value, String>,
) -> Result<Value, String> {
    let handle = match &args[0] {
        Value::Handle(h) => h,
        other => return Err(type_error(name, 0, "プロセス", other)),
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::Process(p) => f(p),
        HandleKind::Closed => Ok(closed()),
        _ => Err(format!(
            "{name} に渡せるのは os.spawn が返したプロセスです（ほかの種類のハンドルでした）"
        )),
    }
}

/// `{"status": 終了コード, "ok": 0 かどうか}`。シグナルで終わったときの終了コードは -1
fn result_value(code: Option<i32>) -> Value {
    let code = code.unwrap_or(-1);
    let mut map = Map::default();
    map.insert("status".into(), Value::Num(f64::from(code)));
    map.insert("ok".into(), Value::Bool(code == 0));
    new_map(map)
}

/// 終わっていれば結果を覚えて返す。まだなら `None`
fn try_finish(p: &mut ProcessHandle) -> Result<Option<Option<i32>>, io::Error> {
    if let Some(code) = p.result {
        return Ok(Some(code));
    }
    Ok(p.child.try_wait()?.map(|status| {
        let code = status.code();
        p.result = Some(code);
        code
    }))
}

fn wait_failure(e: &io::Error) -> Value {
    let (kind, reason) = super::fs::classify(e);
    error_value(kind, format!("プロセスを待てません: {reason}"))
}

pub fn wait(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "os.wait の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let timeout = match args.get(1) {
        None => None,
        Some(Value::Num(_)) => {
            let n = num_arg("os.wait", args, 1)?;
            if !(n.is_finite() && n > 0.0) {
                return Err(format!("os.wait の `timeout` は 0 より大きい数です（{n}）"));
            }
            Some(Duration::try_from_secs_f64(n).unwrap_or(Duration::MAX))
        }
        Some(other) => return Err(type_error("os.wait", 1, "数値", other)),
    };
    with_process("os.wait", args, |p| {
        let started = Instant::now();
        loop {
            match try_finish(p) {
                Ok(Some(code)) => return Ok(result_value(code)),
                Ok(None) => {}
                Err(e) => return Ok(wait_failure(&e)),
            }
            match timeout {
                Some(t) if started.elapsed() >= t => return Ok(Value::Nil),
                _ => thread::sleep(Duration::from_millis(5)),
            }
        }
    })
}

pub fn poll(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.poll", args, 1)?;
    with_process("os.poll", args, |p| {
        Ok(match try_finish(p) {
            Ok(Some(code)) => result_value(code),
            Ok(None) => Value::Nil,
            Err(e) => wait_failure(&e),
        })
    })
}

pub fn kill(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.kill", args, 1)?;
    with_process("os.kill", args, |p| {
        match try_finish(p) {
            Ok(Some(_)) => return Ok(Value::Nil),
            Ok(None) => {}
            Err(e) => return Ok(wait_failure(&e)),
        }
        // 殺してから、終わるのを待つ
        let _ = p.child.kill();
        Ok(match p.child.wait() {
            Ok(status) => {
                p.result = Some(status.code());
                Value::Nil
            }
            Err(e) => wait_failure(&e),
        })
    })
}
