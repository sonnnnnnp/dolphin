//! 標準ライブラリ（ネイティブ関数）。モジュールごとに register で登録する
//!
//! 関数の一覧と仕様は design.md 5.5

pub mod array;
pub mod io;
pub mod map;
pub mod math;
pub mod string;

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    io::register(interp);
    string::register(interp);
    array::register(interp);
    map::register(interp);
    math::register(interp);
}

// ---- 引数の検査（エラーはメッセージだけ返し、位置は Interpreter が付ける） ----

fn expect_args(name: &str, args: &[Value], n: usize) -> Result<(), String> {
    if args.len() == n {
        Ok(())
    } else {
        Err(format!(
            "{name} の引数は {n} 個です（{} 個渡されました）",
            args.len()
        ))
    }
}

/// `i` 番目（0 始まり）の引数の型が違う
fn type_error(name: &str, i: usize, expected: &str, got: &Value) -> String {
    format!(
        "{name} の {} 番目の引数は{expected}です（{}が渡されました）",
        i + 1,
        got.type_name()
    )
}

fn num_arg(name: &str, args: &[Value], i: usize) -> Result<f64, String> {
    match &args[i] {
        Value::Num(n) => Ok(*n),
        other => Err(type_error(name, i, "数値", other)),
    }
}

fn int_arg(name: &str, args: &[Value], i: usize) -> Result<i64, String> {
    let n = num_arg(name, args, i)?;
    if n.fract() == 0.0 {
        Ok(n as i64)
    } else {
        Err(format!(
            "{name} の {} 番目の引数は整数です（{n} が渡されました）",
            i + 1
        ))
    }
}

fn str_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v str, String> {
    match &args[i] {
        Value::Str(s) => Ok(s),
        other => Err(type_error(name, i, "文字列", other)),
    }
}

fn array_arg<'v>(
    name: &str,
    args: &'v [Value],
    i: usize,
) -> Result<&'v Rc<RefCell<Vec<Value>>>, String> {
    match &args[i] {
        Value::Array(items) => Ok(items),
        other => Err(type_error(name, i, "配列", other)),
    }
}

fn map_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<RefCell<Map>>, String> {
    match &args[i] {
        Value::Map(map) => Ok(map),
        other => Err(type_error(name, i, "マップ", other)),
    }
}

fn new_array(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}
