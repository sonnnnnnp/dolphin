//! 標準ライブラリ（ネイティブ関数）。モジュールごとに register で登録する
//!
//! 関数の一覧と仕様は design.md 5.5

pub mod array;
pub mod async_;
#[cfg(feature = "audio")]
pub mod audio;
pub mod bits;
pub mod bytes;
pub mod collections;
#[cfg(feature = "crypto")]
pub mod crypto;
pub mod csv;
pub mod debug;
pub mod encoding;
pub mod error;
pub mod fs;
pub mod gc;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub mod http_server;
#[cfg(feature = "image")]
pub mod image_;
pub mod io;
pub mod json;
pub mod map;
pub mod math;
pub mod net;
pub mod os;
pub mod path;
pub mod process;
pub mod random;
#[cfg(feature = "regex")]
pub mod regex;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod stream;
pub mod string;
pub mod time;
pub mod types;
pub mod worker;
pub mod ws;
pub mod ws_proto;

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    io::register(interp);
    error::register(interp);
    json::register(interp);
    #[cfg(feature = "http")]
    http::register(interp);
    fs::register(interp);
    string::register(interp);
    stream::register(interp);
    array::register(interp);
    map::register(interp);
    math::register(interp);
    bits::register(interp);
    net::register(interp);
    async_::register(interp);
    #[cfg(feature = "audio")]
    audio::register(interp);
    #[cfg(feature = "ui")]
    crate::ui::register(interp);
    ws::register(interp);
    worker::register(interp);
    types::register(interp);
    gc::register(interp);
    debug::register(interp);
    bytes::register(interp);
    encoding::register(interp);
    #[cfg(feature = "crypto")]
    crypto::register(interp);
    collections::register(interp);
    csv::register(interp);
    path::register(interp);
    random::register(interp);
    #[cfg(feature = "image")]
    image_::register(interp);
    #[cfg(feature = "regex")]
    regex::register(interp);
    #[cfg(feature = "sqlite")]
    sqlite::register(interp);
    os::register(interp);
    time::register(interp);
}

// ---- 引数の検査（エラーはメッセージだけ返し、位置は Interpreter が付ける） ----

pub(crate) fn expect_args(name: &str, args: &[Value], n: usize) -> Result<(), String> {
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
pub(crate) fn type_error(name: &str, i: usize, expected: &str, got: &Value) -> String {
    format!(
        "{name} の {} 番目の引数は{expected}です（{}が渡されました）",
        i + 1,
        got.type_name()
    )
}

pub(crate) fn num_arg(name: &str, args: &[Value], i: usize) -> Result<f64, String> {
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

pub(crate) fn str_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v str, String> {
    match &args[i] {
        Value::Str(s) => Ok(s),
        other => Err(type_error(name, i, "文字列", other)),
    }
}

fn bytes_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<[u8]>, String> {
    match &args[i] {
        Value::Bytes(bytes) => Ok(bytes),
        other => Err(type_error(name, i, "バイト列", other)),
    }
}

/// バイト列か文字列（UTF-8 として扱う）を、バイトの並びとして受け取る
fn data_arg<'v>(
    name: &str,
    args: &'v [Value],
    i: usize,
) -> Result<std::borrow::Cow<'v, [u8]>, String> {
    match &args[i] {
        Value::Bytes(bytes) => Ok(std::borrow::Cow::Borrowed(&bytes[..])),
        Value::Str(s) => Ok(std::borrow::Cow::Borrowed(s.as_bytes())),
        other => Err(type_error(name, i, "バイト列か文字列", other)),
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

pub(crate) fn func_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Value, String> {
    match &args[i] {
        f @ (Value::Func(_) | Value::Native(_)) => Ok(f),
        other => Err(type_error(name, i, "関数", other)),
    }
}

fn map_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<RefCell<Map>>, String> {
    match &args[i] {
        Value::Map(map) => Ok(map),
        other => Err(type_error(name, i, "マップ", other)),
    }
}

/// 失敗したときに返すエラー値（design.md 8.6）。引数の型の間違いなどプログラムの間違いは、
/// これではなく `Err(メッセージ)` で実行時エラーにする
fn error_value(kind: &str, message: impl Into<String>) -> Value {
    Value::error(kind, message.into())
}

pub(crate) fn new_map(map: Map) -> Value {
    crate::runtime::gc::new_map(map)
}

fn new_array(items: Vec<Value>) -> Value {
    crate::runtime::gc::new_array(items)
}
