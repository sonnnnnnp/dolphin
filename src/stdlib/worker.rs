//! `std:worker`: ワーカー（design.md 5.43）。別のスレッドに別のインタプリタを立てて、
//! メッセージ（`Data`）でやり取りする。標準ライブラリだけを使う

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, mpsc::Receiver};
use std::thread;
use std::time::{Duration, Instant};

use super::net::closed;
use super::{error_value, new_map, str_arg, type_error};
use crate::Session;
use crate::runtime::data::Data;
use crate::runtime::handle::{Handle, HandleKind};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;
use crate::runtime::worker::{ChildMsg, WorkerContext, WorkerHandle, WorkerOut};

/// 同時に動かせるワーカーの数
const MAX_WORKERS: usize = 64;
/// ワーカーのスレッドのスタック（`dolphin` の本体と同じ。再帰の深さの上限を保つため）
const STACK_SIZE: usize = 256 * 1024 * 1024;

static ALIVE: AtomicUsize = AtomicUsize::new(0);

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:worker",
        &[
            ("worker.spawn", spawn),
            ("worker.send", send),
            ("worker.recv", recv),
            ("worker.join", join),
            ("worker.terminate", terminate),
            ("worker.arg", arg),
            ("worker.is_worker", is_worker),
        ],
    );
}

// ---- 親の側: 起動 ----

/// ワーカーのスレッドが終わるとき、数を戻す
struct AliveGuard;

impl Drop for AliveGuard {
    fn drop(&mut self) {
        ALIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

fn is_function(value: &Value) -> bool {
    matches!(value, Value::Func(_) | Value::Native(_))
}

fn spawn(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "worker.spawn の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let path = str_arg("worker.spawn", args, 0)?;
    let mut arg = Data::Nil;
    let mut callbacks = false;
    if let Some(options) = args.get(1) {
        let Value::Map(options) = options else {
            return Err(type_error("worker.spawn", 1, "マップ", options));
        };
        for (key, value) in options.borrow().iter() {
            match &**key {
                "arg" => arg = Data::from_value(value),
                "on_message" | "on_close" if is_function(value) => callbacks = true,
                "on_message" | "on_close" => {
                    return Err(format!(
                        "worker.spawn の `{key}` は関数です（{}が渡されました）",
                        value.type_name()
                    ));
                }
                _ => {
                    return Err(format!(
                        "worker.spawn に知らないオプション `{key}` があります（使えるのは arg、on_message、on_close）"
                    ));
                }
            }
        }
    }

    let full = interp.current_dir().join(Path::new(path));
    let source = match std::fs::read_to_string(&full) {
        Ok(source) => source,
        Err(e) => {
            let (kind, reason) = if e.kind() == io::ErrorKind::InvalidData {
                ("parse", "UTF-8 として読めません".to_string())
            } else {
                super::fs::classify(&e)
            };
            return Ok(error_value(
                kind,
                format!("`{path}` を読み込めません: {reason}"),
            ));
        }
    };
    if ALIVE.load(Ordering::SeqCst) >= MAX_WORKERS {
        return Err(format!("同時に動かせるワーカーは {MAX_WORKERS} 個までです"));
    }

    let (to_tx, to_rx) = mpsc::channel::<Data>();
    let mut handle = WorkerHandle {
        label: path.to_string(),
        to_child: Some(to_tx),
        from_child: None,
        queue: Default::default(),
        finished: None,
        event_id: None,
    };
    let out;
    let value;
    if callbacks {
        value = Value::Handle(Handle::new(HandleKind::Worker(handle)));
        let id = interp.events.register_stream(
            args[1].clone(),
            value.clone(),
            Arc::new(AtomicBool::new(false)),
        )?;
        if let Value::Handle(h) = &value
            && let HandleKind::Worker(w) = &mut *h.inner.borrow_mut()
        {
            w.event_id = Some(id);
        }
        out = WorkerOut::Events {
            tx: interp.events.sender(),
            id,
        };
    } else {
        let (tx, rx) = mpsc::channel::<ChildMsg>();
        handle.from_child = Some(rx);
        value = Value::Handle(Handle::new(HandleKind::Worker(handle)));
        out = WorkerOut::Channel(tx);
    }

    let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = path.to_string();
    ALIVE.fetch_add(1, Ordering::SeqCst);
    let context = WorkerContext {
        out: out.clone(),
        inbox: to_rx,
        arg,
    };
    let started = thread::Builder::new()
        .name("dolphin-worker".into())
        .stack_size(STACK_SIZE)
        .spawn(move || run_worker(&source, &name, dir, context));
    if let Err(e) = started {
        ALIVE.fetch_sub(1, Ordering::SeqCst);
        if let Value::Handle(h) = &value
            && let Some(id) = match &*h.inner.borrow() {
                HandleKind::Worker(w) => w.event_id,
                _ => None,
            }
        {
            interp.events.cancel(id);
        }
        return Ok(error_value(
            "io",
            format!("ワーカーのスレッドを作れません: {e}"),
        ));
    }
    Ok(value)
}

// ---- ワーカーの中身 ----

fn run_worker(source: &str, name: &str, dir: std::path::PathBuf, context: WorkerContext) {
    let _guard = AliveGuard;
    let out = context.out.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        // ワーカーの `log` は標準出力に直接書く
        let mut stdout = io::stdout();
        let mut session = Session::with_base_dir(&mut stdout, dir);
        session.set_worker(context);
        match session.run(source) {
            Ok(()) => (session.exit_code().unwrap_or(0), String::new()),
            Err(diag) => (1, diag.render(name, source)),
        }
    }));
    let (code, reason) = result.unwrap_or_else(|_| (1, "ワーカーが異常終了しました".to_string()));
    out.exit(code, reason);
}

// ---- 共通 ----

fn timeout_arg(name: &str, args: &[Value], i: usize) -> Result<Duration, String> {
    let n = super::num_arg(name, args, i)?;
    if n.is_finite() && n > 0.0 {
        Ok(Duration::try_from_secs_f64(n).unwrap_or(Duration::MAX))
    } else {
        Err(format!("{name} の `timeout` は 0 より大きい数です（{n}）"))
    }
}

fn not_in_worker(name: &str) -> String {
    format!("{name} はワーカーの中でだけ使えます")
}

/// 親のハンドルに `f` を適用する。終わった・閉じたワーカーはエラー値、ワーカーでなければ実行時エラー
fn with_worker(
    name: &str,
    handle: &Value,
    f: impl FnOnce(&mut WorkerHandle) -> Result<Value, String>,
) -> Result<Value, String> {
    let Value::Handle(handle) = handle else {
        return Err(type_error(name, 0, "ワーカー", handle));
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::Worker(w) => f(w),
        HandleKind::WorkerClosed | HandleKind::Closed => Ok(closed()),
        _ => Err(format!(
            "{name} に渡せるのは worker.spawn が返したワーカーです（ほかの種類のハンドルでした）"
        )),
    }
}

// ---- 送る・受け取る ----

fn send(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    match args.len() {
        // ワーカーの中から親へ
        1 => {
            let ctx = interp
                .worker
                .as_ref()
                .ok_or_else(|| not_in_worker("worker.send[data]"))?;
            Ok(if ctx.out.post(Data::from_value(&args[0])) {
                Value::Nil
            } else {
                closed()
            })
        }
        // 親からワーカーへ
        2 => with_worker("worker.send", &args[0], |w| {
            let sent = w
                .to_child
                .as_ref()
                .is_some_and(|tx| tx.send(Data::from_value(&args[1])).is_ok());
            Ok(if sent { Value::Nil } else { closed() })
        }),
        n => Err(format!(
            "worker.send の引数は、ワーカーの中なら 1 個、親なら 2 個です（{n} 個渡されました）"
        )),
    }
}

fn recv(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if args.len() > 2 {
        return Err(format!(
            "worker.recv の引数は 0〜2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    match args.first() {
        // ワーカーの中: recv[] / recv[timeout]
        None | Some(Value::Num(_)) => {
            let timeout = match args.first() {
                Some(_) => Some(timeout_arg("worker.recv", args, 0)?),
                None => None,
            };
            let ctx = interp
                .worker
                .as_ref()
                .ok_or_else(|| not_in_worker("worker.recv[]"))?;
            Ok(match timeout {
                None => ctx.inbox.recv().map_or(Value::Nil, Data::into_value),
                Some(t) => match ctx.inbox.recv_timeout(t) {
                    Ok(data) => data.into_value(),
                    Err(RecvTimeoutError::Timeout) => {
                        error_value("timeout", "時間内にメッセージが来ませんでした")
                    }
                    Err(RecvTimeoutError::Disconnected) => Value::Nil,
                },
            })
        }
        // 親: recv[w] / recv[w, timeout]
        Some(handle) => {
            let timeout = match args.get(1) {
                Some(_) => Some(timeout_arg("worker.recv", args, 1)?),
                None => None,
            };
            with_worker("worker.recv", handle, |w| parent_recv(w, timeout))
        }
    }
}

/// ブロッキング型の `recv`。届いたものを順に、終わっていれば `nil`
fn parent_recv(w: &mut WorkerHandle, timeout: Option<Duration>) -> Result<Value, String> {
    if w.from_child.is_none() {
        return Err(
            "worker.recv はブロッキング型のワーカーに使います（コールバック型でした）".into(),
        );
    }
    let started = Instant::now();
    loop {
        if let Some(data) = w.queue.pop_front() {
            return Ok(data.into_value());
        }
        if w.finished.is_some() {
            return Ok(Value::Nil);
        }
        let rx = w.from_child.as_ref().expect("上で確かめた");
        let next = match timeout {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(t) => rx.recv_timeout(t.saturating_sub(started.elapsed())),
        };
        match next {
            Ok(ChildMsg::Message(data)) => w.queue.push_back(data),
            Ok(ChildMsg::Exit(code, reason)) => w.finished = Some((code, reason)),
            Err(RecvTimeoutError::Timeout) => {
                return Ok(error_value("timeout", "時間内にメッセージが来ませんでした"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                w.finished = Some((1, "ワーカーが予期せず終わりました".into()));
            }
        }
    }
}

fn join(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "worker.join の引数は 1 個です（{} 個渡されました）",
            args.len()
        ));
    }
    with_worker("worker.join", &args[0], |w| {
        if w.from_child.is_none() {
            return Err(
                "worker.join はブロッキング型のワーカーに使います（コールバック型でした）".into(),
            );
        }
        while w.finished.is_none() {
            let rx: &Receiver<ChildMsg> = w.from_child.as_ref().expect("上で確かめた");
            match rx.recv() {
                Ok(ChildMsg::Message(data)) => w.queue.push_back(data),
                Ok(ChildMsg::Exit(code, reason)) => w.finished = Some((code, reason)),
                Err(_) => w.finished = Some((1, "ワーカーが予期せず終わりました".into())),
            }
        }
        let (code, reason) = w.finished.clone().expect("上で待った");
        let mut map = Map::default();
        map.insert("ok".into(), Value::Bool(code == 0));
        map.insert("status".into(), Value::Num(f64::from(code)));
        map.insert(
            "error".into(),
            if reason.is_empty() {
                Value::Nil
            } else {
                Value::Str(reason.into())
            },
        );
        Ok(new_map(map))
    })
}

fn terminate(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "worker.terminate の引数は 1 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let Value::Handle(handle) = &args[0] else {
        return Err(type_error("worker.terminate", 0, "ワーカー", &args[0]));
    };
    let id = match &mut *handle.inner.borrow_mut() {
        HandleKind::Worker(w) => {
            w.to_child = None;
            w.event_id
        }
        HandleKind::WorkerClosed | HandleKind::Closed => return Ok(Value::Nil),
        _ => {
            return Err(
                "worker.terminate に渡せるのは worker.spawn が返したワーカーです（ほかの種類のハンドルでした）"
                    .into(),
            );
        }
    };
    if let Some(id) = id {
        // 以降のコールバックを呼ばない（ハンドルも閉じた扱いになる）
        interp.events.cancel(id);
    }
    handle.close();
    Ok(Value::Nil)
}

// ---- ワーカーの中 ----

fn arg(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    super::expect_args("worker.arg", args, 0)?;
    let ctx = interp
        .worker
        .as_ref()
        .ok_or_else(|| not_in_worker("worker.arg"))?;
    Ok(ctx.arg.clone().into_value())
}

fn is_worker(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    super::expect_args("worker.is_worker", args, 0)?;
    Ok(Value::Bool(interp.worker.is_some()))
}
