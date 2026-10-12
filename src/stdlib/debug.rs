//! 開発の道具: assert, assert_eq, dbg（design.md 5.17）

use super::expect_args;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::{Repr, Value};

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("assert", assert);
    interp.register("assert_eq", assert_eq);
    interp.register("dbg", dbg);
}

fn expect_args_between(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(format!(
            "{name} の引数は {min} 個か {max} 個です（{} 個渡されました）",
            args.len()
        ))
    }
}

/// 最後の引数の `message`（省略できる）。文字列でなければ表示用の文字列にする
fn message(args: &[Value], i: usize) -> Option<String> {
    args.get(i).map(Value::to_string)
}

fn assert(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args_between("assert", args, 1, 2)?;
    match &args[0] {
        Value::Bool(true) => Ok(Value::Nil),
        Value::Bool(false) => {
            Err(message(args, 1).unwrap_or_else(|| "assert に失敗しました".into()))
        }
        other => Err(super::type_error("assert", 0, "真偽値", other)),
    }
}

fn assert_eq(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args_between("assert_eq", args, 2, 3)?;
    if args[0].equals(&args[1]) {
        return Ok(Value::Nil);
    }
    let head = message(args, 2).unwrap_or_else(|| "assert_eq に失敗しました".into());
    Err(format!(
        "{head}\n  左: {}\n  右: {}",
        Repr(&args[0]),
        Repr(&args[1])
    ))
}

/// 値を標準エラーに出して、そのまま返す
fn dbg(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("dbg", args, 1)?;
    // 標準出力（`log`）と混ざらないよう、バッファに溜まった分を先に流す
    interp.out.flush().map_err(|e| e.to_string())?;
    eprintln!("[dbg] {}", Repr(&args[0]));
    Ok(args[0].clone())
}
