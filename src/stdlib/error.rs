//! エラー値: is_error, err_kind, err_message, err_new（design.md 8.6）

use super::{expect_args, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::{ErrorValue, Value};

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("is_error", is_error);
    interp.register("err_kind", err_kind);
    interp.register("err_message", err_message);
    interp.register("err_new", err_new);
}

fn is_error(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("is_error", args, 1)?;
    Ok(Value::Bool(matches!(args[0], Value::Error(_))))
}

/// エラー値の `kind`（`"not_found"` など）
fn err_kind(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("err_kind", args, 1)?;
    Ok(Value::Str(error_arg("err_kind", args)?.kind.clone()))
}

fn err_message(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("err_message", args, 1)?;
    Ok(Value::Str(error_arg("err_message", args)?.message.clone()))
}

/// 自分でエラー値を作る
fn err_new(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("err_new", args, 2)?;
    let kind = str_arg("err_new", args, 0)?;
    let message = str_arg("err_new", args, 1)?;
    Ok(Value::error(kind, message))
}

fn error_arg<'v>(name: &str, args: &'v [Value]) -> Result<&'v ErrorValue, String> {
    match &args[0] {
        Value::Error(e) => Ok(e),
        other => Err(super::type_error(name, 0, "エラー値", other)),
    }
}
