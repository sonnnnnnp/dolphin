//! マップ: map_len, map_keys, map_has, map_remove（design.md 5.4）

use std::rc::Rc;

use super::{expect_args, map_arg, new_array, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("map_len", map_len);
    interp.register("map_keys", map_keys);
    interp.register("map_has", map_has);
    interp.register("map_remove", map_remove);
}

fn map_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_len", args, 1)?;
    let map = map_arg("map_len", args, 0)?;
    Ok(Value::Num(map.borrow().len() as f64))
}

/// キーを追加順に並べた配列
fn map_keys(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_keys", args, 1)?;
    let map = map_arg("map_keys", args, 0)?;
    let keys = map
        .borrow()
        .iter()
        .map(|(k, _)| Value::Str(Rc::clone(k)))
        .collect();
    Ok(new_array(keys))
}

fn map_has(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_has", args, 2)?;
    let map = map_arg("map_has", args, 0)?;
    let key = str_arg("map_has", args, 1)?;
    Ok(Value::Bool(map.borrow().contains_key(key)))
}

/// 取り除いた値を返す。キーがなければエラー
fn map_remove(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_remove", args, 2)?;
    let map = map_arg("map_remove", args, 0)?;
    let key = str_arg("map_remove", args, 1)?;
    map.borrow_mut()
        .remove(key)
        .ok_or_else(|| format!("キー {key:?} がありません"))
}
