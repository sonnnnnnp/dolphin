//! マップ（design.md 5.4、5.12）。その場で書き換えるのは `map_remove` と `map_clear` だけ

use std::rc::Rc;

use super::{expect_args, map_arg, new_array, new_map, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("map_len", map_len);
    interp.register("map_keys", map_keys);
    interp.register("map_has", map_has);
    interp.register("map_remove", map_remove);
    interp.register("map_values", map_values);
    interp.register("map_items", map_items);
    interp.register("map_get", map_get);
    interp.register("map_merge", map_merge);
    interp.register("map_clear", map_clear);
    interp.register("map_copy", map_copy);
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

/// 値を追加順に並べた配列
fn map_values(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_values", args, 1)?;
    let map = map_arg("map_values", args, 0)?;
    let values = map.borrow().iter().map(|(_, v)| v.clone()).collect();
    Ok(new_array(values))
}

/// `{キー, 値}` の配列を追加順に並べた配列
fn map_items(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_items", args, 1)?;
    let map = map_arg("map_items", args, 0)?;
    let items = map
        .borrow()
        .iter()
        .map(|(k, v)| new_array(vec![Value::Str(Rc::clone(k)), v.clone()]))
        .collect();
    Ok(new_array(items))
}

/// キーがあれば値、なければ `default`
fn map_get(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_get", args, 3)?;
    let map = map_arg("map_get", args, 0)?;
    let key = str_arg("map_get", args, 1)?;
    let value = map.borrow().get(key).cloned();
    Ok(value.unwrap_or_else(|| args[2].clone()))
}

/// `a` に `b` を重ねた新しいマップ。同じキーは `b` が勝つ（位置は `a` のまま）
fn map_merge(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_merge", args, 2)?;
    let mut merged = map_arg("map_merge", args, 0)?.borrow().clone();
    for (k, v) in map_arg("map_merge", args, 1)?.borrow().iter() {
        merged.insert(Rc::clone(k), v.clone());
    }
    Ok(new_map(merged))
}

fn map_clear(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_clear", args, 1)?;
    map_arg("map_clear", args, 0)?.borrow_mut().clear();
    Ok(Value::Nil)
}

/// 浅いコピー。値の配列やマップは、元と同じものを指す
fn map_copy(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("map_copy", args, 1)?;
    Ok(new_map(map_arg("map_copy", args, 0)?.borrow().clone()))
}
