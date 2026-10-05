//! 文字列: str_len, str_concat

use super::{expect_args, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("str_len", str_len);
    interp.register("str_concat", str_concat);
}

/// 文字数（バイト数ではない）
fn str_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_len", args, 1)?;
    match &args[0] {
        Value::Str(s) => Ok(Value::Num(s.chars().count() as f64)),
        other => Err(type_error("str_len", "文字列", other)),
    }
}

/// 値を文字列にしてつなげる。数値も渡せる
fn str_concat(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let s: String = args.iter().map(Value::to_string).collect();
    Ok(Value::Str(s.into()))
}
