//! ファイル: file_read, file_write, file_append, file_exists（design.md 5.5）
//!
//! パスは実行したときのカレントディレクトリからの相対パス（`import` と違い、
//! 呼び出したファイルのあるディレクトリではない）。文字コードは UTF-8

use std::fs::{self, OpenOptions};
use std::io::{self, Write};

use super::{expect_args, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("file_read", file_read);
    interp.register("file_write", file_write);
    interp.register("file_append", file_append);
    interp.register("file_exists", file_exists);
}

/// ファイル全体を文字列で返す
fn file_read(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_read", args, 1)?;
    let path = str_arg("file_read", args, 0)?;
    let bytes = fs::read(path).map_err(|e| io_error("読み込めません", path, &e))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("`{path}` を読み込めません: UTF-8 として読めません"))?;
    Ok(Value::Str(text.into()))
}

/// 文字列を書き込む。ファイルがなければ作り、あれば中身を置き換える
fn file_write(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_write", args, 2)?;
    let path = str_arg("file_write", args, 0)?;
    let text = str_arg("file_write", args, 1)?;
    fs::write(path, text).map_err(|e| io_error("書き込めません", path, &e))?;
    Ok(Value::Nil)
}

/// 文字列をファイルの末尾に足す。ファイルがなければ作る
fn file_append(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_append", args, 2)?;
    let path = str_arg("file_append", args, 0)?;
    let text = str_arg("file_append", args, 1)?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| io_error("書き込めません", path, &e))?;
    Ok(Value::Nil)
}

/// ファイルかディレクトリがあるか
fn file_exists(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_exists", args, 1)?;
    let path = str_arg("file_exists", args, 0)?;
    Ok(Value::Bool(fs::exists(path).unwrap_or(false)))
}

/// OS の言語に左右されないよう、よくある原因は固定の文言にする
fn io_error(action: &str, path: &str, e: &io::Error) -> String {
    let reason = match e.kind() {
        io::ErrorKind::NotFound => "ファイルが見つかりません".to_string(),
        io::ErrorKind::PermissionDenied => "権限がありません".to_string(),
        _ => e.to_string(),
    };
    format!("`{path}` を{action}: {reason}")
}
