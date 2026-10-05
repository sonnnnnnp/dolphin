//! マップ: map_len, map_keys, map_has, map_remove（design.md 5.4）

use std::cell::RefCell;
use std::rc::Rc;

use super::{expect_args, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("map_len", map_len);
    interp.register("map_keys", map_keys);
    interp.register("map_has", map_has);
    interp.register("map_remove", map_remove);
}

fn as_map<'v>(name: &str, value: &'v Value) -> Result<&'v Rc<RefCell<Map>>, String> {
    match value {
        Value::Map(map) => Ok(map),
        other => Err(type_error(name, "マップ", other)),
    }
}

fn as_key<'v>(name: &str, value: &'v Value) -> Result<&'v str, String> {
    match value {
        Value::Str(s) => Ok(s),
        other => Err(type_error(name, "文字列のキー", other)),
    }
}

fn map_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_len", args, 1)?;
    let map = as_map("map_len", &args[0])?;
    Ok(Value::Num(map.borrow().len() as f64))
}

/// キーを追加順に並べた配列
fn map_keys(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_keys", args, 1)?;
    let map = as_map("map_keys", &args[0])?;
    let keys = map
        .borrow()
        .iter()
        .map(|(k, _)| Value::Str(Rc::clone(k)))
        .collect();
    Ok(Value::Array(Rc::new(RefCell::new(keys))))
}

fn map_has(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_has", args, 2)?;
    let map = as_map("map_has", &args[0])?;
    let key = as_key("map_has", &args[1])?;
    Ok(Value::Bool(map.borrow().contains_key(key)))
}

/// 取り除いた値を返す。キーがなければエラー
fn map_remove(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_remove", args, 2)?;
    let map = as_map("map_remove", &args[0])?;
    let key = as_key("map_remove", &args[1])?;
    map.borrow_mut()
        .remove(key)
        .ok_or_else(|| format!("キー {key:?} がありません"))
}
