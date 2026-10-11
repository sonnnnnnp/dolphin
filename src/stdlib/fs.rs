//! ファイル: file_read, file_write, file_append, file_exists（design.md 5.5）、
//! file_list, file_info, file_mkdir, file_remove, file_remove_all, file_rename, file_copy（5.13）
//!
//! パスは実行したときのカレントディレクトリからの相対パス（`import` と違い、
//! 呼び出したファイルのあるディレクトリではない）。文字コードは UTF-8。
//! 失敗したときはエラー値を返す（design.md 8.6）。種類は `not_found` `permission` `exists` `io` `parse`

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::time::UNIX_EPOCH;

use super::{error_value, expect_args, new_array, new_map, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("file_read", file_read);
    interp.register("file_write", file_write);
    interp.register("file_append", file_append);
    interp.register("file_exists", file_exists);
    interp.register("file_list", file_list);
    interp.register("file_info", file_info);
    interp.register("file_mkdir", file_mkdir);
    interp.register("file_remove", file_remove);
    interp.register("file_remove_all", file_remove_all);
    interp.register("file_rename", file_rename);
    interp.register("file_copy", file_copy);
}

/// ファイル全体を文字列で返す
fn file_read(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_read", args, 1)?;
    let path = str_arg("file_read", args, 0)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => return Ok(io_error("読み込めません", path, &e)),
    };
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Value::Str(text.into())),
        Err(_) => Ok(error_value(
            "parse",
            format!("`{path}` を読み込めません: UTF-8 として読めません"),
        )),
    }
}

/// 文字列を書き込む。ファイルがなければ作り、あれば中身を置き換える
fn file_write(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_write", args, 2)?;
    let path = str_arg("file_write", args, 0)?;
    let text = str_arg("file_write", args, 1)?;
    match fs::write(path, text) {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error("書き込めません", path, &e)),
    }
}

/// 文字列をファイルの末尾に足す。ファイルがなければ作る
fn file_append(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_append", args, 2)?;
    let path = str_arg("file_append", args, 0)?;
    let text = str_arg("file_append", args, 1)?;
    let result = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()));
    match result {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error("書き込めません", path, &e)),
    }
}

/// ファイルかディレクトリがあるか
fn file_exists(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_exists", args, 1)?;
    let path = str_arg("file_exists", args, 0)?;
    Ok(Value::Bool(fs::exists(path).unwrap_or(false)))
}

/// OS の言語に左右されないよう、よくある原因は固定の文言にする
fn io_error(action: &str, path: &str, e: &io::Error) -> Value {
    let (kind, reason) = classify(e);
    error_value(kind, format!("`{path}` を{action}: {reason}"))
}

/// 2 つのパスを使う操作（名前の変更・コピー）の失敗
fn io_error2(action: &str, from: &str, to: &str, e: &io::Error) -> Value {
    let (kind, reason) = classify(e);
    error_value(kind, format!("`{from}` を `{to}` に{action}: {reason}"))
}

/// エラーの種類（`kind`）と、メッセージの理由
pub(super) fn classify(e: &io::Error) -> (&'static str, String) {
    match e.kind() {
        io::ErrorKind::NotFound => ("not_found", "ファイルが見つかりません".to_string()),
        io::ErrorKind::PermissionDenied => ("permission", "権限がありません".to_string()),
        io::ErrorKind::AlreadyExists => ("exists", "すでにあります".to_string()),
        io::ErrorKind::DirectoryNotEmpty => ("io", "ディレクトリが空ではありません".to_string()),
        io::ErrorKind::NotADirectory => ("io", "ディレクトリではありません".to_string()),
        io::ErrorKind::IsADirectory => ("io", "ディレクトリです".to_string()),
        _ => ("io", e.to_string()),
    }
}

/// ディレクトリの中の名前を、文字コード順に並べた配列
fn file_list(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_list", args, 1)?;
    let path = str_arg("file_list", args, 0)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return Ok(io_error("開けません", path, &e)),
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(e) => return Ok(io_error("読み込めません", path, &e)),
        }
    }
    names.sort_unstable();
    Ok(new_array(
        names.into_iter().map(|n| Value::Str(n.into())).collect(),
    ))
}

/// `{"is_file", "is_dir", "size", "modified"}`。シンボリックリンクはたどる
fn file_info(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_info", args, 1)?;
    let path = str_arg("file_info", args, 0)?;
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) => return Ok(io_error("調べられません", path, &e)),
    };
    // 1970 年より前なら負の数。取れない環境では nil
    let modified = match meta.modified() {
        Ok(time) => match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Value::Num(after.as_secs_f64()),
            Err(before) => Value::Num(-before.duration().as_secs_f64()),
        },
        Err(_) => Value::Nil,
    };
    let mut info = Map::default();
    info.insert("is_file".into(), Value::Bool(meta.is_file()));
    info.insert("is_dir".into(), Value::Bool(meta.is_dir()));
    info.insert("size".into(), Value::Num(meta.len() as f64));
    info.insert("modified".into(), modified);
    Ok(new_map(info))
}

/// ディレクトリを作る。途中のディレクトリもまとめて作り、すでにあれば何もしない
fn file_mkdir(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_mkdir", args, 1)?;
    let path = str_arg("file_mkdir", args, 0)?;
    match fs::create_dir_all(path) {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error("作れません", path, &e)),
    }
}

/// ファイルか空のディレクトリを消す
fn file_remove(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_remove", args, 1)?;
    let path = str_arg("file_remove", args, 0)?;
    let result = fs::symlink_metadata(path).and_then(|meta| {
        if meta.is_dir() {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    });
    match result {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error("消せません", path, &e)),
    }
}

/// ディレクトリを中身ごと消す。ファイルも消せる。リンクは、リンク先をたどらずにリンクだけを消す
fn file_remove_all(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_remove_all", args, 1)?;
    let path = str_arg("file_remove_all", args, 0)?;
    let result = fs::symlink_metadata(path).and_then(|meta| {
        if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    });
    match result {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error("消せません", path, &e)),
    }
}

/// 名前を変える・移す。`to` が既にあるファイルなら置き換える
fn file_rename(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_rename", args, 2)?;
    let from = str_arg("file_rename", args, 0)?;
    let to = str_arg("file_rename", args, 1)?;
    match fs::rename(from, to) {
        Ok(()) => Ok(Value::Nil),
        Err(e) => Ok(io_error2("移せません", from, to, &e)),
    }
}

/// ファイルをコピーする（ディレクトリは不可）。`to` が既にあれば置き換える
fn file_copy(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("file_copy", args, 2)?;
    let from = str_arg("file_copy", args, 0)?;
    let to = str_arg("file_copy", args, 1)?;
    match fs::copy(from, to) {
        Ok(_) => Ok(Value::Nil),
        Err(e) => Ok(io_error2("コピーできません", from, to, &e)),
    }
}
