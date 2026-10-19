//! 型: type_of（design.md 5.12）

use super::expect_args;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("type_of", type_of);
}

/// 型の名前の文字列。判定は `type_of[@x] == "number"` と書く
fn type_of(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("type_of", args, 1)?;
    let name = match &args[0] {
        Value::Nil => "nil",
        Value::Bool(_) => "bool",
        Value::Num(_) => "number",
        Value::Int(_) => "int",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Func(_) | Value::Native(_) => "function",
        Value::Module(_) => "module",
        Value::Error(_) => "error",
        Value::Bytes(_) => "bytes",
        Value::Handle(_) => "handle",
    };
    Ok(Value::Str(name.into()))
}
