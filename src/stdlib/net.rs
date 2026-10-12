//! `std:net`: TCP（design.md 5.21）。ブロッキングで、標準ライブラリだけを使う

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::time::Duration;

use super::{data_arg, error_value, expect_args, int_arg, new_map, num_arg, str_arg, type_error};
use crate::runtime::handle::{Handle, HandleKind};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

/// 1 行の長さの上限
const MAX_LINE: usize = 16 * 1024 * 1024;
/// `read` が一度に読める大きさの上限
const MAX_READ: i64 = 64 * 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: f64 = 30.0;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:net",
        &[
            ("net.connect", connect),
            ("net.listen", listen),
            ("net.accept", accept),
            ("net.read", read),
            ("net.read_exact", read_exact),
            ("net.read_line", read_line),
            ("net.write", write),
            ("net.close", close),
            ("net.set_timeout", set_timeout),
            ("net.local_addr", local_addr),
            ("net.peer_addr", peer_addr),
        ],
    );
}

// ---- 引数・エラー ----

fn handle_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<Handle>, String> {
    match &args[i] {
        Value::Handle(h) => Ok(h),
        other => Err(type_error(name, i, "ハンドル", other)),
    }
}

fn port_arg(name: &str, args: &[Value], i: usize) -> Result<u16, String> {
    let n = int_arg(name, args, i)?;
    u16::try_from(n).map_err(|_| format!("{name} のポートは 0〜65535 の整数です（{n}）"))
}

pub(super) fn closed() -> Value {
    error_value("closed", "ハンドルはすでに閉じています")
}

/// 通信の失敗をエラー値にする
pub(super) fn failure(what: &str, e: &io::Error) -> Value {
    let kind = match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "timeout",
        _ => "network",
    };
    error_value(kind, format!("{what}: {e}"))
}

pub(super) fn seconds(name: &str, n: f64) -> Result<Duration, String> {
    if n.is_finite() && n > 0.0 {
        Ok(Duration::from_secs_f64(n))
    } else {
        Err(format!("{name} の秒数は 0 より大きい数です（{n}）"))
    }
}

fn conn_handle(stream: TcpStream) -> io::Result<Value> {
    // 小さな書き込みをまとめて待たせない
    stream.set_nodelay(true)?;
    let writer = stream.try_clone()?;
    Ok(Value::Handle(Handle::new(HandleKind::TcpConn {
        reader: BufReader::new(stream),
        writer,
    })))
}

fn addr_map(addr: SocketAddr) -> Value {
    let mut map = Map::default();
    map.insert("host".into(), Value::Str(addr.ip().to_string().into()));
    map.insert("port".into(), Value::Num(f64::from(addr.port())));
    new_map(map)
}

// ---- 接続 ----

fn connect(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "net.connect の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let host = str_arg("net.connect", args, 0)?;
    let port = port_arg("net.connect", args, 1)?;
    let mut timeout = DEFAULT_CONNECT_TIMEOUT;
    if let Some(options) = args.get(2) {
        let Value::Map(options) = options else {
            return Err(type_error("net.connect", 2, "マップ", options));
        };
        for (key, value) in options.borrow().iter() {
            match (&**key, value) {
                ("timeout", Value::Num(n)) => timeout = *n,
                ("timeout", other) => {
                    return Err(format!(
                        "net.connect のオプション `timeout` は数値です（{}が渡されました）",
                        other.type_name()
                    ));
                }
                _ => return Err(format!("net.connect のオプション `{key}` は知りません")),
            }
        }
    }
    let timeout = seconds("net.connect", timeout)?;

    let addrs = match (host, port).to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(e) => return Ok(failure(&format!("{host} を引けません"), &e)),
    };
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                return match conn_handle(stream) {
                    Ok(value) => Ok(value),
                    Err(e) => Ok(failure("接続を準備できません", &e)),
                };
            }
            Err(e) => last = Some(e),
        }
    }
    Ok(match last {
        Some(e) => failure(&format!("{host}:{port} に接続できません"), &e),
        None => error_value("network", format!("{host} のアドレスが見つかりません")),
    })
}

fn listen(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.listen", args, 1)?;
    let (host, port) = match &args[0] {
        Value::Num(_) => ("127.0.0.1".to_string(), port_arg("net.listen", args, 0)?),
        Value::Map(options) => {
            let options = options.borrow();
            let mut host = "127.0.0.1".to_string();
            let mut port = None;
            for (key, value) in options.iter() {
                match (&**key, value) {
                    ("host", Value::Str(s)) => host = s.to_string(),
                    ("port", Value::Num(n)) => {
                        port = Some(
                            u16::try_from(*n as i64)
                                .ok()
                                .filter(|_| n.fract() == 0.0)
                                .ok_or_else(|| {
                                    format!("net.listen のポートは 0〜65535 の整数です（{n}）")
                                })?,
                        );
                    }
                    ("host" | "port", other) => {
                        return Err(format!(
                            "net.listen のオプション `{key}` の型が違います（{}が渡されました）",
                            other.type_name()
                        ));
                    }
                    _ => return Err(format!("net.listen のオプション `{key}` は知りません")),
                }
            }
            let port = port.ok_or("net.listen に `port` がありません")?;
            (host, port)
        }
        other => return Err(type_error("net.listen", 0, "数値かマップ", other)),
    };
    Ok(match TcpListener::bind((host.as_str(), port)) {
        Ok(listener) => Value::Handle(Handle::new(HandleKind::TcpListener(listener))),
        Err(e) => failure(&format!("{host}:{port} で待ち受けられません"), &e),
    })
}

fn accept(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.accept", args, 1)?;
    let handle = handle_arg("net.accept", args, 0)?;
    let inner = handle.inner.borrow();
    match &*inner {
        HandleKind::TcpListener(listener) => Ok(match listener.accept() {
            Ok((stream, _)) => {
                conn_handle(stream).unwrap_or_else(|e| failure("接続を準備できません", &e))
            }
            Err(e) => failure("接続を受け付けられません", &e),
        }),
        HandleKind::Closed => Ok(closed()),
        HandleKind::TcpConn { .. } | HandleKind::WsClient { .. } | HandleKind::WsServer { .. } => {
            Err("net.accept に渡せるのは待ち受けのハンドルです（接続でした）".into())
        }
    }
}

// ---- 読み書き ----

/// 接続のハンドルに `f` を適用する。閉じていればエラー値、待ち受けなら実行時エラー
fn with_conn(
    name: &str,
    args: &[Value],
    f: impl FnOnce(&mut BufReader<TcpStream>, &mut TcpStream) -> Result<Value, String>,
) -> Result<Value, String> {
    let handle = handle_arg(name, args, 0)?;
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::TcpConn { reader, writer } => f(reader, writer),
        HandleKind::Closed => Ok(closed()),
        HandleKind::TcpListener(_) => Err(format!(
            "{name} に渡せるのは接続のハンドルです（待ち受けでした）"
        )),
        HandleKind::WsClient { .. } | HandleKind::WsServer { .. } => Err(format!(
            "{name} に渡せるのは TCP の接続のハンドルです（WebSocket でした）"
        )),
    }
}

fn size_arg(name: &str, args: &[Value], i: usize) -> Result<usize, String> {
    let n = int_arg(name, args, i)?;
    if (0..=MAX_READ).contains(&n) {
        Ok(n as usize)
    } else {
        Err(format!("{name} のバイト数は 0〜{MAX_READ} です（{n}）"))
    }
}

fn read(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.read", args, 2)?;
    let n = size_arg("net.read", args, 1)?;
    with_conn("net.read", args, |reader, _| {
        if n == 0 {
            return Ok(Value::bytes([]));
        }
        Ok(match reader.fill_buf() {
            Ok(buf) => {
                let len = buf.len().min(n);
                let bytes = Value::bytes(&buf[..len]);
                reader.consume(len);
                bytes
            }
            Err(e) => failure("読み込みに失敗しました", &e),
        })
    })
}

fn read_exact(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.read_exact", args, 2)?;
    let n = size_arg("net.read_exact", args, 1)?;
    with_conn("net.read_exact", args, |reader, _| {
        let mut buf = vec![0; n];
        Ok(match reader.read_exact(&mut buf) {
            Ok(()) => Value::bytes(buf),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                error_value("eof", format!("{n} バイト読む前に接続が閉じました"))
            }
            Err(e) => failure("読み込みに失敗しました", &e),
        })
    })
}

fn read_line(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.read_line", args, 1)?;
    with_conn("net.read_line", args, |reader, _| {
        let mut buf = Vec::new();
        let read = reader
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf);
        match read {
            Err(e) => return Ok(failure("読み込みに失敗しました", &e)),
            Ok(0) => return Ok(Value::Nil),
            Ok(_) => {}
        }
        if buf.len() > MAX_LINE {
            return Ok(error_value("network", "1 行が長すぎます（16 MB まで）"));
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
    })
}

fn write(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.write", args, 2)?;
    let data = data_arg("net.write", args, 1)?;
    with_conn("net.write", args, |_, writer| {
        Ok(
            match writer.write_all(&data).and_then(|()| writer.flush()) {
                Ok(()) => Value::Nil,
                Err(e) => failure("書き込みに失敗しました", &e),
            },
        )
    })
}

fn close(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.close", args, 1)?;
    handle_arg("net.close", args, 0)?.close();
    Ok(Value::Nil)
}

fn set_timeout(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.set_timeout", args, 2)?;
    let timeout = match &args[1] {
        Value::Nil => None,
        Value::Num(_) => Some(seconds(
            "net.set_timeout",
            num_arg("net.set_timeout", args, 1)?,
        )?),
        other => return Err(type_error("net.set_timeout", 1, "数値か nil", other)),
    };
    with_conn("net.set_timeout", args, |reader, writer| {
        // 読み込みと書き込みで別のソケット（複製）を持つので、それぞれに設定する
        let result = reader
            .get_ref()
            .set_read_timeout(timeout)
            .and_then(|()| writer.set_write_timeout(timeout));
        Ok(match result {
            Ok(()) => Value::Nil,
            Err(e) => failure("時間切れを設定できません", &e),
        })
    })
}

fn local_addr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.local_addr", args, 1)?;
    let handle = handle_arg("net.local_addr", args, 0)?;
    let addr = match &*handle.inner.borrow() {
        HandleKind::TcpConn { writer, .. } | HandleKind::WsClient { writer, .. } => {
            writer.local_addr()
        }
        HandleKind::TcpListener(listener) => listener.local_addr(),
        HandleKind::WsServer { .. } => {
            return Err(
                "net.local_addr に渡せるのは TCP のハンドルです（WebSocket でした）".into(),
            );
        }
        HandleKind::Closed => return Ok(closed()),
    };
    Ok(addr.map_or_else(|e| failure("アドレスを取れません", &e), addr_map))
}

fn peer_addr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("net.peer_addr", args, 1)?;
    with_conn("net.peer_addr", args, |_, writer| {
        Ok(writer
            .peer_addr()
            .map_or_else(|e| failure("アドレスを取れません", &e), addr_map))
    })
}
