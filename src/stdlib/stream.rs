//! `std:stream`: ストリーム（design.md 5.41）。ファイル・標準入出力・HTTP の本文・メモリを、
//! 少しずつ読み書きする。標準ライブラリだけを使う

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Write};
use std::rc::Rc;

use super::fs::classify;
use super::net::closed;
use super::{
    bytes_arg, data_arg, error_value, expect_args, func_arg, int_arg, str_arg, type_error,
};
use crate::runtime::handle::{Handle, HandleKind, StreamIo, StreamSink};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

/// 1 行の長さの上限
const MAX_LINE: usize = 16 * 1024 * 1024;
/// `read` が一度に読める大きさの上限
const MAX_READ: i64 = 64 * 1024 * 1024;
/// `read_all` `read_text` が読める大きさの上限
const MAX_ALL: u64 = 1 << 30;
/// `copy` が一度に移す大きさ
const CHUNK: usize = 64 * 1024;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:stream",
        &[
            ("stream.open", open),
            ("stream.create", create),
            ("stream.append", append),
            ("stream.stdin", stdin),
            ("stream.stdout", stdout),
            ("stream.stderr", stderr),
            ("stream.from_bytes", from_bytes),
            ("stream.read", read),
            ("stream.read_exact", read_exact),
            ("stream.read_line", read_line),
            ("stream.read_all", read_all),
            ("stream.read_text", read_text),
            ("stream.each_line", each_line),
            ("stream.write", write),
            ("stream.flush", flush),
            ("stream.copy", copy),
            ("stream.close", close),
        ],
    );
}

/// 読む向きと書く向きの口。ストリームは片方、TCP の接続は両方を持つ
struct Io<'a> {
    reader: Option<&'a mut dyn BufRead>,
    writer: Option<&'a mut dyn Write>,
    /// 失敗を `network` の種類にするか
    network: bool,
}

impl<'a> Io<'a> {
    fn reader(&mut self, name: &str) -> Result<&mut (dyn BufRead + 'a), String> {
        match self.reader.as_mut() {
            Some(reader) => Ok(&mut **reader),
            None => Err(format!(
                "{name} は読み込み用のストリームに使います（書き込み用でした）"
            )),
        }
    }

    fn writer(&mut self, name: &str) -> Result<&mut (dyn Write + 'a), String> {
        match self.writer.as_mut() {
            Some(writer) => Ok(&mut **writer),
            None => Err(format!(
                "{name} は書き込み用のストリームに使います（読み込み用でした）"
            )),
        }
    }

    fn failure(&self, what: &str, e: &io::Error) -> Value {
        let kind = match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "timeout",
            io::ErrorKind::NotFound => "not_found",
            io::ErrorKind::PermissionDenied => "permission",
            _ if self.network => "network",
            _ => "io",
        };
        error_value(kind, format!("{what}: {e}"))
    }
}

/// ストリームか TCP の接続のハンドルに `f` を適用する。閉じていればエラー値、
/// ほかの種類のハンドルなら実行時エラー
fn with_io(
    interp: &mut Interpreter<'_>,
    name: &str,
    handle: &Handle,
    f: impl FnOnce(&mut Io<'_>) -> Result<Value, String>,
) -> Result<Value, String> {
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::Stream(stream) => {
            let writer: Option<&mut dyn Write> = match &mut stream.writer {
                Some(StreamSink::Writer(w)) => Some(&mut **w),
                Some(StreamSink::InterpOut) => Some(&mut *interp.out),
                None => None,
            };
            f(&mut Io {
                reader: match &mut stream.reader {
                    Some(r) => Some(&mut **r),
                    None => None,
                },
                writer,
                network: stream.network,
            })
        }
        HandleKind::TcpConn { reader, writer } => f(&mut Io {
            reader: Some(reader),
            writer: Some(writer),
            network: true,
        }),
        HandleKind::StreamClosed | HandleKind::Closed => Ok(closed()),
        _ => Err(format!(
            "{name} に渡せるのはストリームか TCP の接続のハンドルです（ほかの種類のハンドルでした）"
        )),
    }
}

fn handle_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<Handle>, String> {
    match &args[i] {
        Value::Handle(h) => Ok(h),
        other => Err(type_error(name, i, "ハンドル", other)),
    }
}

pub(super) fn stream_value(
    reader: Option<Box<dyn BufRead>>,
    writer: Option<StreamSink>,
    label: String,
) -> Value {
    Value::Handle(Handle::new(HandleKind::Stream(StreamIo {
        reader,
        writer,
        label,
        network: false,
    })))
}

/// 外から作った読み込みのストリーム（HTTP の本文）
#[cfg(feature = "http")]
pub fn network_reader(reader: Box<dyn BufRead>, label: String) -> Value {
    Value::Handle(Handle::new(HandleKind::Stream(StreamIo {
        reader: Some(reader),
        writer: None,
        label,
        network: true,
    })))
}

// ---- 作る ----

fn file_error(action: &str, path: &str, e: &io::Error) -> Value {
    let (kind, reason) = classify(e);
    error_value(kind, format!("`{path}` を{action}: {reason}"))
}

fn open(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.open", args, 1)?;
    let path = str_arg("stream.open", args, 0)?;
    Ok(match File::open(path) {
        Ok(file) => stream_value(
            Some(Box::new(BufReader::new(file))),
            None,
            format!("ファイル {path} 読み込み"),
        ),
        Err(e) => file_error("開けません", path, &e),
    })
}

fn create(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.create", args, 1)?;
    let path = str_arg("stream.create", args, 0)?;
    Ok(match File::create(path) {
        Ok(file) => stream_value(
            None,
            Some(StreamSink::Writer(Box::new(BufWriter::new(file)))),
            format!("ファイル {path} 書き込み"),
        ),
        Err(e) => file_error("作れません", path, &e),
    })
}

fn append(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.append", args, 1)?;
    let path = str_arg("stream.append", args, 0)?;
    Ok(
        match OpenOptions::new().append(true).create(true).open(path) {
            Ok(file) => stream_value(
                None,
                Some(StreamSink::Writer(Box::new(BufWriter::new(file)))),
                format!("ファイル {path} 追記"),
            ),
            Err(e) => file_error("開けません", path, &e),
        },
    )
}

fn stdin(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.stdin", args, 0)?;
    Ok(stream_value(
        Some(Box::new(io::stdin().lock())),
        None,
        "標準入力".into(),
    ))
}

fn stdout(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.stdout", args, 0)?;
    Ok(stream_value(
        None,
        Some(StreamSink::InterpOut),
        "標準出力".into(),
    ))
}

fn stderr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.stderr", args, 0)?;
    Ok(stream_value(
        None,
        Some(StreamSink::Writer(Box::new(io::stderr()))),
        "標準エラー".into(),
    ))
}

fn from_bytes(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.from_bytes", args, 1)?;
    let bytes = bytes_arg("stream.from_bytes", args, 0)?;
    Ok(stream_value(
        Some(Box::new(Cursor::new(bytes.to_vec()))),
        None,
        "メモリ".into(),
    ))
}

// ---- 読む ----

fn size_arg(name: &str, args: &[Value], i: usize) -> Result<usize, String> {
    let n = int_arg(name, args, i)?;
    if (0..=MAX_READ).contains(&n) {
        Ok(n as usize)
    } else {
        Err(format!("{name} のバイト数は 0〜{MAX_READ} です（{n}）"))
    }
}

/// 届いている分を最大 `max` バイト取り出す。終わりなら空のバイト列
fn take_chunk(io: &mut Io<'_>, name: &str, max: usize) -> Result<Value, String> {
    let reader = io.reader(name)?;
    if max == 0 {
        return Ok(Value::bytes([]));
    }
    let result = reader.fill_buf().map(|buf| {
        let len = buf.len().min(max);
        (Value::bytes(&buf[..len]), len)
    });
    match result {
        Ok((bytes, len)) => {
            reader.consume(len);
            Ok(bytes)
        }
        Err(e) => Ok(io.failure("読み込みに失敗しました", &e)),
    }
}

fn read(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.read", args, 2)?;
    let n = size_arg("stream.read", args, 1)?;
    let handle = handle_arg("stream.read", args, 0)?;
    with_io(interp, "stream.read", handle, |io| {
        take_chunk(io, "stream.read", n)
    })
}

fn read_exact(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.read_exact", args, 2)?;
    let n = size_arg("stream.read_exact", args, 1)?;
    let handle = handle_arg("stream.read_exact", args, 0)?;
    with_io(interp, "stream.read_exact", handle, |io| {
        let mut buf = vec![0; n];
        let result = io.reader("stream.read_exact")?.read_exact(&mut buf);
        Ok(match result {
            Ok(()) => Value::bytes(buf),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                error_value("eof", format!("{n} バイト読む前に終わりました"))
            }
            Err(e) => io.failure("読み込みに失敗しました", &e),
        })
    })
}

/// 改行までの 1 行。終わりなら `nil`
fn read_line_from(io: &mut Io<'_>, name: &str) -> Result<Value, String> {
    let mut buf = Vec::new();
    let result = io
        .reader(name)?
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf);
    match result {
        Err(e) => return Ok(io.failure("読み込みに失敗しました", &e)),
        Ok(0) => return Ok(Value::Nil),
        Ok(_) => {}
    }
    if buf.len() > MAX_LINE {
        return Ok(error_value("too_large", "1 行が長すぎます（16 MB まで）"));
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    }
    Ok(match String::from_utf8(buf) {
        Ok(line) => Value::Str(line.into()),
        Err(_) => error_value("parse", "UTF-8 として読めない行です"),
    })
}

fn read_line(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.read_line", args, 1)?;
    let handle = handle_arg("stream.read_line", args, 0)?;
    with_io(interp, "stream.read_line", handle, |io| {
        read_line_from(io, "stream.read_line")
    })
}

/// 残りをすべて読む。多すぎれば `too_large` のエラー値
fn read_rest(io: &mut Io<'_>, name: &str) -> Result<Result<Vec<u8>, Value>, String> {
    let mut buf = Vec::new();
    let result = io.reader(name)?.take(MAX_ALL + 1).read_to_end(&mut buf);
    Ok(match result {
        Err(e) => Err(io.failure("読み込みに失敗しました", &e)),
        Ok(_) if buf.len() as u64 > MAX_ALL => {
            Err(error_value("too_large", "大きすぎます（1 GiB まで）"))
        }
        Ok(_) => Ok(buf),
    })
}

fn read_all(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.read_all", args, 1)?;
    let handle = handle_arg("stream.read_all", args, 0)?;
    with_io(interp, "stream.read_all", handle, |io| {
        Ok(match read_rest(io, "stream.read_all")? {
            Ok(buf) => Value::bytes(buf),
            Err(error) => error,
        })
    })
}

fn read_text(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.read_text", args, 1)?;
    let handle = handle_arg("stream.read_text", args, 0)?;
    with_io(interp, "stream.read_text", handle, |io| {
        Ok(match read_rest(io, "stream.read_text")? {
            Ok(buf) => match String::from_utf8(buf) {
                Ok(text) => Value::Str(text.into()),
                Err(_) => error_value("parse", "UTF-8 として読めません"),
            },
            Err(error) => error,
        })
    })
}

fn each_line(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.each_line", args, 2)?;
    let handle = handle_arg("stream.each_line", args, 0)?;
    let f = func_arg("stream.each_line", args, 1)?;
    loop {
        let line = with_io(interp, "stream.each_line", handle, |io| {
            read_line_from(io, "stream.each_line")
        })?;
        match line {
            Value::Nil => return Ok(Value::Nil),
            Value::Str(_) => {
                if let Value::Bool(false) = interp.call_value(f, vec![line])? {
                    return Ok(Value::Nil);
                }
            }
            error => return Ok(error),
        }
    }
}

// ---- 書く・閉じる ----

fn write(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.write", args, 2)?;
    let data = data_arg("stream.write", args, 1)?;
    let handle = handle_arg("stream.write", args, 0)?;
    with_io(interp, "stream.write", handle, |io| {
        let result = io.writer("stream.write")?.write_all(&data);
        Ok(match result {
            Ok(()) => Value::Nil,
            Err(e) => io.failure("書き込みに失敗しました", &e),
        })
    })
}

fn flush(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.flush", args, 1)?;
    let handle = handle_arg("stream.flush", args, 0)?;
    with_io(interp, "stream.flush", handle, |io| {
        let result = io.writer("stream.flush")?.flush();
        Ok(match result {
            Ok(()) => Value::Nil,
            Err(e) => io.failure("書き出しに失敗しました", &e),
        })
    })
}

fn copy(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.copy", args, 2)?;
    let from = handle_arg("stream.copy", args, 0)?;
    let to = handle_arg("stream.copy", args, 1)?;
    let mut total = 0u64;
    loop {
        let chunk = with_io(interp, "stream.copy", from, |io| {
            take_chunk(io, "stream.copy", CHUNK)
        })?;
        let Value::Bytes(bytes) = chunk else {
            return Ok(chunk);
        };
        if bytes.is_empty() {
            return Ok(Value::Num(total as f64));
        }
        total += bytes.len() as u64;
        let written = with_io(interp, "stream.copy", to, |io| {
            let result = io.writer("stream.copy")?.write_all(&bytes);
            Ok(match result {
                Ok(()) => Value::Nil,
                Err(e) => io.failure("書き込みに失敗しました", &e),
            })
        })?;
        if !matches!(written, Value::Nil) {
            return Ok(written);
        }
    }
}

fn close(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("stream.close", args, 1)?;
    let handle = handle_arg("stream.close", args, 0)?;
    let already = matches!(
        &*handle.inner.borrow(),
        HandleKind::StreamClosed | HandleKind::Closed
    );
    if already {
        return Ok(Value::Nil);
    }
    // 書き込みは、書き残しを書き出してから閉じる（失敗しても閉じる）
    let flushed = with_io(interp, "stream.close", handle, |io| {
        let result = match io.writer.as_mut() {
            Some(writer) => writer.flush(),
            None => Ok(()),
        };
        Ok(match result {
            Ok(()) => Value::Nil,
            Err(e) => io.failure("書き出しに失敗しました", &e),
        })
    })?;
    handle.close();
    Ok(match flushed {
        Value::Error(_) => flushed,
        _ => Value::Nil,
    })
}
