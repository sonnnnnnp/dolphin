//! メモリ回収: gc_collect（design.md 5.15）

use super::expect_args;
use crate::runtime::gc;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("gc_collect", gc_collect);
}

/// すぐに循環回収器を動かし、回収した対象の数を返す
fn gc_collect(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("gc_collect", args, 0)?;
    Ok(Value::Num(gc::collect() as f64))
}
