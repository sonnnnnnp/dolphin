//! 配列: arr_len, arr_push

use super::{expect_args, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("arr_len", arr_len);
    interp.register("arr_push", arr_push);
}

fn arr_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_len", args, 1)?;
    match &args[0] {
        Value::Array(items) => Ok(Value::Num(items.borrow().len() as f64)),
        other => Err(type_error("arr_len", "配列", other)),
    }
}

/// 末尾に追加する。呼び出し元の配列が書き換わるかは design.md 7.1 で決める
fn arr_push(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_push", args, 2)?;
    match &args[0] {
        Value::Array(items) => {
            items.borrow_mut().push(args[1].clone());
            Ok(Value::Nil)
        }
        other => Err(type_error("arr_push", "配列", other)),
    }
}
