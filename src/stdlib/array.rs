//! 配列: arr_len, arr_push, arr_pop

use super::{array_arg, expect_args};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("arr_len", arr_len);
    interp.register("arr_push", arr_push);
    interp.register("arr_pop", arr_pop);
}

fn arr_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_len", args, 1)?;
    let items = array_arg("arr_len", args, 0)?;
    Ok(Value::Num(items.borrow().len() as f64))
}

/// 末尾に追加する。配列は参照渡しなので、呼び出し元の配列が変わる（design.md 7.1）
fn arr_push(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_push", args, 2)?;
    let items = array_arg("arr_push", args, 0)?;
    items.borrow_mut().push(args[1].clone());
    Ok(Value::Nil)
}

/// 末尾を取り除いて返す。空ならエラー
fn arr_pop(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_pop", args, 1)?;
    let items = array_arg("arr_pop", args, 0)?;
    items
        .borrow_mut()
        .pop()
        .ok_or_else(|| "空の配列からは取り出せません".to_string())
}
