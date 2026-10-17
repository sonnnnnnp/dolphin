//! エラー値: is_error, err_kind, err_message, err_new, err_data（design.md 8.6・5.39）

use super::{expect_args, str_arg};
use crate::runtime::data::Data;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::{ErrorValue, Value};

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("is_error", is_error);
    interp.register("err_kind", err_kind);
    interp.register("err_message", err_message);
    interp.register("err_new", err_new);
    interp.register("err_data", err_data);
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
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "err_new の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let kind = str_arg("err_new", args, 0)?;
    let message = str_arg("err_new", args, 1)?;
    // `nil` は「詳細なし」と同じ
    let data = args
        .get(2)
        .filter(|v| !matches!(v, Value::Nil))
        .map(Data::from_value);
    Ok(Value::error_with(kind, message, data))
}

/// エラー値の詳細データ。なければ `nil`
fn err_data(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("err_data", args, 1)?;
    Ok(match &error_arg("err_data", args)?.data {
        Some(data) => (**data).clone().into_value(),
        None => Value::Nil,
    })
}

fn error_arg<'v>(name: &str, args: &'v [Value]) -> Result<&'v ErrorValue, String> {
    match &args[0] {
        Value::Error(e) => Ok(e),
        other => Err(super::type_error(name, 0, "エラー値", other)),
    }
}
