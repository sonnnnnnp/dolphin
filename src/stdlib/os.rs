//! `std:os`: args, env, env_all, cwd, chdir, exit, platform, temp_dir, run（design.md 5.13）

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::path::to_slash;
use super::{error_value, expect_args, int_arg, new_array, new_map, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:os",
        &[
            ("os.args", args),
            ("os.env", env),
            ("os.env_all", env_all),
            ("os.cwd", cwd),
            ("os.chdir", chdir),
            ("os.exit", exit),
            ("os.platform", platform),
            ("os.temp_dir", temp_dir),
            ("os.run", run),
        ],
    );
}

fn string(s: impl Into<String>) -> Value {
    Value::Str(s.into().into())
}

/// スクリプトに渡されたコマンドライン引数
fn args(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.args", args, 0)?;
    Ok(new_array(interp.args.iter().map(string).collect()))
}

/// 環境変数の値。なければ `nil`
fn env(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.env", args, 1)?;
    let name = str_arg("os.env", args, 0)?;
    // 名前に `=` や NUL があると、OS の関数が受け付けない
    if name.is_empty() || name.contains(['=', '\0']) {
        return Ok(Value::Nil);
    }
    Ok(std::env::var_os(name).map_or(Value::Nil, |v| string(v.to_string_lossy())))
}

/// 環境変数すべてのマップ（名前順）
fn env_all(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.env_all", args, 0)?;
    let vars: BTreeMap<String, String> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let mut map = Map::default();
    for (k, v) in vars {
        map.insert(k.into(), string(v));
    }
    Ok(new_map(map))
}

fn cwd(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.cwd", args, 0)?;
    match std::env::current_dir() {
        Ok(dir) => Ok(string(to_slash(&dir))),
        Err(e) => Ok(io_failure("カレントディレクトリを調べられません", None, &e)),
    }
}

/// 移動する。ファイル関数の相対パスの基準も変わる
fn chdir(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.chdir", args, 1)?;
    let path = str_arg("os.chdir", args, 0)?;
    match std::env::set_current_dir(path) {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_failure("移動できません", Some(path), &e)),
    }
}

/// その場でスクリプトを終了する。出力は書き出してから終わる
fn exit(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if args.len() > 1 {
        return Err(format!(
            "os.exit の引数は 0 個か 1 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let code = if args.is_empty() {
        0
    } else {
        int_arg("os.exit", args, 0)?
    };
    let code =
        i32::try_from(code).map_err(|_| format!("os.exit の終了コード {code} は大きすぎます"))?;
    interp.out.flush().map_err(|e| e.to_string())?;
    interp.exit_code = Some(code);
    // 呼び出し側（Interpreter）が、終了の合図として受け取る
    Err("os.exit".into())
}

fn platform(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.platform", args, 0)?;
    Ok(string(std::env::consts::OS))
}

fn temp_dir(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("os.temp_dir", args, 0)?;
    Ok(string(to_slash(&std::env::temp_dir())))
}

/// OS のエラーを、種類つきのエラー値にする
fn io_failure(action: &str, path: Option<&str>, e: &io::Error) -> Value {
    let (kind, reason) = super::fs::classify(e);
    let subject = path.map_or(String::new(), |p| format!("`{p}` を"));
    error_value(kind, format!("{subject}{action}: {reason}"))
}

/// `run` の `options`
struct RunOptions {
    cwd: Option<String>,
    env: Vec<(String, String)>,
    stdin: Option<String>,
    timeout: Option<Duration>,
}

fn run_options(value: &Value) -> Result<RunOptions, String> {
    let Value::Map(options) = value else {
        return Err(type_error("os.run", 2, "マップ", value));
    };
    let options = options.borrow();
    const KNOWN: [&str; 4] = ["cwd", "env", "stdin", "timeout"];
    for (key, _) in options.iter() {
        if !KNOWN.contains(&&**key) {
            return Err(format!(
                "os.run に知らないオプション `{key}` があります（使えるのは {}）",
                KNOWN.join("、")
            ));
        }
    }
    let text = |key: &str| -> Result<Option<String>, String> {
        match options.get(key) {
            None => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.to_string())),
            Some(other) => Err(format!(
                "os.run の `{key}` は文字列です（{}が渡されました）",
                other.type_name()
            )),
        }
    };
    let mut env = Vec::new();
    match options.get("env") {
        None => {}
        Some(Value::Map(map)) => {
            for (key, value) in map.borrow().iter() {
                let Value::Str(value) = value else {
                    return Err(format!(
                        "os.run の `env` の `{key}` は文字列です（{}が渡されました）",
                        value.type_name()
                    ));
                };
                if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
                    return Err(format!("os.run の `env` の `{key}` は環境変数にできません"));
                }
                env.push((key.to_string(), value.to_string()));
            }
        }
        Some(other) => {
            return Err(format!(
                "os.run の `env` はマップです（{}が渡されました）",
                other.type_name()
            ));
        }
    }
    let timeout = match options.get("timeout") {
        None => None,
        Some(Value::Num(n)) if n.is_finite() && *n > 0.0 => {
            Some(Duration::try_from_secs_f64(*n).unwrap_or(Duration::MAX))
        }
        Some(Value::Num(n)) => {
            return Err(format!("os.run の `timeout` は 0 より大きい数です（{n}）"));
        }
        Some(other) => {
            return Err(format!(
                "os.run の `timeout` は数値です（{}が渡されました）",
                other.type_name()
            ));
        }
    };
    Ok(RunOptions {
        cwd: text("cwd")?,
        env,
        stdin: text("stdin")?,
        timeout,
    })
}

/// 外部コマンドを実行して、終わるまで待つ（シェルは通さない）
fn run(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "os.run の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let command = str_arg("os.run", args, 0)?;
    let Value::Array(list) = &args[1] else {
        return Err(type_error("os.run", 1, "配列", &args[1]));
    };
    let mut arguments = Vec::new();
    for (i, item) in list.borrow().iter().enumerate() {
        match item {
            Value::Str(s) if !s.contains('\0') => arguments.push(s.to_string()),
            Value::Str(_) => return Err(format!("os.run の引数 {i} 番目に NUL は使えません")),
            other => {
                return Err(format!(
                    "os.run の引数の配列の {i} 番目は文字列です（{}が渡されました）",
                    other.type_name()
                ));
            }
        }
    }
    let options = match args.get(2) {
        Some(value) => run_options(value)?,
        None => RunOptions {
            cwd: None,
            env: Vec::new(),
            stdin: None,
            timeout: None,
        },
    };
    if command.is_empty() || command.contains('\0') {
        return Err("os.run のコマンド名が正しくありません".into());
    }
    // 子プロセスの出力が、こちらの出力と前後しないように
    interp.out.flush().map_err(|e| e.to_string())?;
    Ok(spawn_and_wait(command, &arguments, options))
}

fn spawn_and_wait(command: &str, arguments: &[String], options: RunOptions) -> Value {
    // 起動に失敗したときの OS のエラーは、システムによって違う（存在しない場所を
    // 指定したとき、Windows は「ディレクトリ名が正しくない」になる）ので、先に調べる
    if let Some(dir) = &options.cwd {
        match std::fs::metadata(dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return error_value("io", format!("`{dir}` はディレクトリではありません"));
            }
            Err(e) => return io_failure("カレントディレクトリにできません", Some(dir), &e),
        }
    }
    let mut cmd = Command::new(command);
    cmd.args(arguments)
        .stdin(if options.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
            return error_value(kind, format!("`{command}` を実行できません: {reason}"));
        }
    };

    // パイプが詰まって子プロセスが止まらないよう、読み書きは別のスレッドで行う
    let stdin_thread = options.stdin.map(|input| {
        let mut pipe = child.stdin.take().expect("stdin は piped にしてある");
        thread::spawn(move || {
            // 子プロセスが先に終わると書き込みに失敗するが、それは問題にしない
            let _ = pipe.write_all(input.as_bytes());
        })
    });
    let out_thread = reader(child.stdout.take().expect("stdout は piped にしてある"));
    let err_thread = reader(child.stderr.take().expect("stderr は piped にしてある"));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        if options.timeout.is_some_and(|t| started.elapsed() >= t) {
            // 殺して、読み取りのスレッドが終わるのを待つ
            let _ = child.kill();
            let _ = child.wait();
            let _ = join(out_thread);
            let _ = join(err_thread);
            return error_value(
                "timeout",
                format!("`{command}` が時間内に終わりませんでした（殺しました）"),
            );
        }
        thread::sleep(Duration::from_millis(5));
    };
    if let Some(t) = stdin_thread {
        let _ = t.join();
    }
    let (stdout, stderr) = (join(out_thread), join(err_thread));
    let status = match status {
        Ok(status) => status,
        Err(e) => return io_failure("待てません", Some(command), &e),
    };

    let text = |bytes: Vec<u8>, name: &str| {
        String::from_utf8(bytes).map_err(|_| {
            error_value(
                "parse",
                format!("`{command}` の{name}が UTF-8 ではありません"),
            )
        })
    };
    let (stdout, stderr) = match (text(stdout, "標準出力"), text(stderr, "標準エラー出力"))
    {
        (Ok(out), Ok(err)) => (out, err),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    // シグナルで終わったときは、終了コードがない
    let code = status.code().unwrap_or(-1);
    let mut result = Map::default();
    result.insert("status".into(), Value::Num(f64::from(code)));
    result.insert("ok".into(), Value::Bool(code == 0));
    result.insert("stdout".into(), string(stdout));
    result.insert("stderr".into(), string(stderr));
    new_map(result)
}

fn reader(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        // 読み取りの失敗は、そこまでの内容を返す
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

fn join(handle: thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    handle.join().unwrap_or_default()
}
