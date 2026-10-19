//! `std:async`: after, every, cancel, run, stop（design.md 5.23）と、非同期の関数の共通処理

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::{expect_args, int_arg, num_arg, type_error};
use crate::runtime::data::Data;
use crate::runtime::events::Event;
use crate::runtime::handle::Handle;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:async",
        &[
            ("async.after", after),
            ("async.every", every),
            ("async.cancel", cancel),
            ("async.run", run),
            ("async.stop", stop),
            ("async.sleep", sleep),
        ],
    );
}

/// `std:async` を `import` したときに足す、Dolphin で書いた関数（design.md 5.46）
pub const PRELUDE: &str = "async all[@futures] (
    @results := {}
    for @f in @futures (
        arr_push[@results, await @f]
    )
    # @results
)
";

/// 最後の引数のコールバックと、残りの引数に分ける
pub fn split_callback<'v>(name: &str, args: &'v [Value]) -> Result<(&'v [Value], Value), String> {
    let Some((last, rest)) = args.split_last() else {
        return Err(format!("{name} の最後の引数はコールバックの関数です"));
    };
    match last {
        Value::Func(_) | Value::Native(_) => Ok((rest, last.clone())),
        other => Err(format!(
            "{name} の最後の引数はコールバックの関数です（{}が渡されました）",
            other.type_name()
        )),
    }
}

/// 最後の引数がコールバックなら、それと残りの引数に分ける。コールバックがなければ、すべて引数
/// （コールバックを省いた非同期の関数は、future を返す。design.md 5.46）
pub fn split_callback_opt(args: &[Value]) -> (&[Value], Option<Value>) {
    match args.split_last() {
        Some((last @ (Value::Func(_) | Value::Native(_)), rest)) => (rest, Some(last.clone())),
        _ => (args, None),
    }
}

/// `spawn_blocking` のコールバックを省ける版。省いたら future を返す
pub fn spawn_blocking_opt(
    interp: &mut Interpreter<'_>,
    callback: Option<Value>,
    work: impl FnOnce() -> Value + Send + 'static,
) -> Result<Value, String> {
    match callback {
        Some(callback) => spawn_blocking(interp, callback, work),
        None => {
            let future = Handle::new_future();
            let id = interp.events.register(future.clone(), None)?;
            let tx = interp.events.sender();
            thread::spawn(move || {
                let data = Data::from_value(&work());
                let _ = tx.send(Event::Done { id, data });
            });
            Ok(future)
        }
    }
}

/// `work` を補助スレッドで実行し、結果を `callback` に渡すように登録する。戻り値は番号。
/// `work` が作る `Value` は、そのスレッドの中で `Data` にしてから送る
pub fn spawn_blocking(
    interp: &mut Interpreter<'_>,
    callback: Value,
    work: impl FnOnce() -> Value + Send + 'static,
) -> Result<Value, String> {
    let id = interp.events.register(callback, None)?;
    let tx = interp.events.sender();
    thread::spawn(move || {
        let data = Data::from_value(&work());
        let _ = tx.send(Event::Done { id, data });
    });
    Ok(Value::Num(id as f64))
}

fn seconds(name: &str, args: &[Value], allow_zero: bool) -> Result<Duration, String> {
    let n = num_arg(name, args, 0)?;
    if n.is_finite() && (n > 0.0 || (allow_zero && n == 0.0)) {
        Ok(Duration::from_secs_f64(n))
    } else {
        let limit = if allow_zero {
            "0 以上の数"
        } else {
            "0 より大きい数"
        };
        Err(format!("{name} の秒数は{limit}です（{n}）"))
    }
}

fn callback_arg(name: &str, args: &[Value], i: usize) -> Result<Value, String> {
    match &args[i] {
        f @ (Value::Func(_) | Value::Native(_)) => Ok(f.clone()),
        other => Err(type_error(name, i, "関数", other)),
    }
}

fn after(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.after", args, 2)?;
    let delay = seconds("async.after", args, true)?;
    let callback = callback_arg("async.after", args, 1)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let id = interp
        .events
        .register(callback, Some((Arc::clone(&cancelled), false)))?;
    let tx = interp.events.sender();
    thread::spawn(move || {
        thread::sleep(delay);
        if !cancelled.load(Ordering::SeqCst) {
            let _ = tx.send(Event::Tick { id });
        }
    });
    Ok(Value::Num(id as f64))
}

fn every(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.every", args, 2)?;
    let interval = seconds("async.every", args, false)?;
    let callback = callback_arg("async.every", args, 1)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let id = interp
        .events
        .register(callback, Some((Arc::clone(&cancelled), true)))?;
    let tx = interp.events.sender();
    thread::spawn(move || {
        loop {
            thread::sleep(interval);
            if cancelled.load(Ordering::SeqCst) || tx.send(Event::Tick { id }).is_err() {
                break;
            }
        }
    });
    Ok(Value::Num(id as f64))
}

/// `seconds` 秒後に `nil` で終わる future
fn sleep(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.sleep", args, 1)?;
    let delay = seconds("async.sleep", args, true)?;
    let future = Handle::new_future();
    let cancelled = Arc::new(AtomicBool::new(false));
    let id = interp
        .events
        .register(future.clone(), Some((Arc::clone(&cancelled), false)))?;
    let tx = interp.events.sender();
    thread::spawn(move || {
        thread::sleep(delay);
        let _ = tx.send(Event::Tick { id });
    });
    Ok(future)
}

fn cancel(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.cancel", args, 1)?;
    let id = int_arg("async.cancel", args, 0)?;
    if let Ok(id) = u64::try_from(id) {
        interp.events.cancel(id);
    }
    Ok(Value::Nil)
}

fn run(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.run", args, 0)?;
    interp.run_event_loop()?;
    Ok(Value::Nil)
}

fn stop(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("async.stop", args, 0)?;
    interp.events.stop();
    Ok(Value::Nil)
}
