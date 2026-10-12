//! `std:http` の `serve`（design.md 5.16）
//!
//! 接続の受け付け、HTTP の解析、応答の書き込みは補助スレッド（接続ごとに 1 つ）で行い、
//! 解析済みのデータ（`RequestData`）だけをインタプリタのスレッドに送る。ハンドラはインタプリタの
//! スレッドで 1 つずつ呼び、応答のデータ（`ResponseData`）を補助スレッドに返す。関数値や `Rc` は
//! スレッドをまたがない（design.md 8.3）。HTTP の解析は自前で、外部クレートは使わない

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use super::json::to_json_string;
use super::{error_value, expect_args, func_arg, new_map, type_error};
use crate::diag::Diagnostic;
use crate::runtime::error::RuntimeError;
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

const DEFAULT_MAX_BODY: usize = 10 * 1024 * 1024;
/// request line とヘッダー全体の大きさの上限
const MAX_HEAD: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 256;
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const TEXT: &str = "text/plain; charset=utf-8";

// ---- スレッドをまたぐデータ ----

struct RequestData {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    remote: String,
}

struct ResponseData {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Job {
    req: RequestData,
    reply: Sender<ResponseData>,
}

// ---- serve（インタプリタのスレッド） ----

pub fn serve(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("http.serve", args, 2)?;
    let (host, port, max_body) = options(args)?;
    let handler = func_arg("http.serve", args, 1)?.clone();

    let listener = match TcpListener::bind((host.as_str(), port)) {
        Ok(listener) => listener,
        Err(e) => {
            return Ok(error_value(
                "network",
                format!("{host}:{port} で待ち受けられません: {e}"),
            ));
        }
    };
    let (jobs, queue) = mpsc::channel::<Job>();
    thread::spawn(move || accept_loop(&listener, &jobs, max_body));

    loop {
        let Ok(job) = queue.recv() else {
            return Ok(error_value("network", "接続の受け付けが止まりました"));
        };
        let response = match handle(interp, &handler, job.req) {
            Ok(response) => response,
            Err(message) => {
                // `os.exit` はサーバーごと止める
                if interp.exit_code.is_some() {
                    return Err(message);
                }
                eprintln!("{message}");
                internal_error()
            }
        };
        // 相手がもう切れていても気にしない
        let _ = job.reply.send(response);
    }
}

/// `serve[port, handler]` か `serve[options, handler]` の `(host, port, max_body)`
fn options(args: &[Value]) -> Result<(String, u16, usize), String> {
    let port_of = |n: f64| -> Result<u16, String> {
        if n.fract() == 0.0 && (0.0..=65535.0).contains(&n) {
            Ok(n as u16)
        } else {
            Err(format!("http.serve のポートは 0〜65535 の整数です（{n}）"))
        }
    };
    match &args[0] {
        Value::Num(n) => Ok(("127.0.0.1".into(), port_of(*n)?, DEFAULT_MAX_BODY)),
        Value::Map(options) => {
            let options = options.borrow();
            for (key, _) in options.iter() {
                if !["port", "host", "max_body"].contains(&&**key) {
                    return Err(format!("http.serve のオプション `{key}` は知りません"));
                }
            }
            let port = match options.get("port") {
                Some(Value::Num(n)) => port_of(*n)?,
                Some(other) => return Err(option_type("port", "数値", other)),
                None => return Err("http.serve に `port` がありません".into()),
            };
            let host = match options.get("host") {
                Some(Value::Str(s)) => s.to_string(),
                Some(other) => return Err(option_type("host", "文字列", other)),
                None => "127.0.0.1".into(),
            };
            let max_body = match options.get("max_body") {
                Some(Value::Num(n)) if *n >= 0.0 && n.fract() == 0.0 => *n as usize,
                Some(other) => return Err(option_type("max_body", "0 以上の整数", other)),
                None => DEFAULT_MAX_BODY,
            };
            Ok((host, port, max_body))
        }
        other => Err(type_error("http.serve", 0, "数値かマップ", other)),
    }
}

fn option_type(key: &str, expected: &str, got: &Value) -> String {
    format!(
        "http.serve のオプション `{key}` は{expected}です（{}が渡されました）",
        got.type_name()
    )
}

/// ハンドラを呼んで、応答のデータにする。失敗は標準エラーに出すメッセージ
fn handle(
    interp: &mut Interpreter<'_>,
    handler: &Value,
    req: RequestData,
) -> Result<ResponseData, String> {
    let value = interp
        .call_value(handler, vec![request_value(req)])
        .map_err(|message| match interp.take_native_error() {
            Some(e) => describe(e),
            None => format!("http.serve: ハンドラでエラー: {message}"),
        })?;
    response_data(&value)
        .map_err(|m| format!("http.serve: ハンドラの戻り値が正しくありません: {m}"))
}

fn describe(e: RuntimeError) -> String {
    let diag = Diagnostic::from(e);
    let mut text = format!(
        "http.serve: ハンドラでエラー: {}:{}: {}",
        diag.span.line, diag.span.col, diag.message
    );
    for note in &diag.notes {
        text.push_str(&format!("\n  {note}"));
    }
    text
}

fn request_value(req: RequestData) -> Value {
    let strings = |pairs: Vec<(String, String)>| {
        let mut map = Map::default();
        for (k, v) in pairs {
            map.insert(k.as_str().into(), Value::Str(v.into()));
        }
        new_map(map)
    };
    let mut map = Map::default();
    map.insert("method".into(), Value::Str(req.method.into()));
    map.insert("path".into(), Value::Str(req.path.into()));
    map.insert("query".into(), strings(req.query));
    map.insert("headers".into(), strings(req.headers));
    let body = match std::str::from_utf8(&req.body) {
        Ok(s) => Value::Str(s.into()),
        Err(_) => Value::Nil,
    };
    map.insert("body".into(), body);
    map.insert("raw".into(), Value::bytes(&req.body));
    map.insert("remote".into(), Value::Str(req.remote.into()));
    new_map(map)
}

fn internal_error() -> ResponseData {
    ResponseData {
        status: 500,
        headers: vec![("content-type".into(), TEXT.into())],
        body: b"Internal Server Error".to_vec(),
    }
}

/// ハンドラの戻り値（文字列かマップ）を応答のデータにする
fn response_data(value: &Value) -> Result<ResponseData, String> {
    let map = match value {
        Value::Str(s) => {
            return Ok(ResponseData {
                status: 200,
                headers: vec![("content-type".into(), TEXT.into())],
                body: s.as_bytes().to_vec(),
            });
        }
        Value::Map(map) => map.borrow(),
        other => {
            return Err(format!(
                "文字列かマップを返してください（{}でした）",
                other.type_name()
            ));
        }
    };
    for (key, _) in map.iter() {
        if !["status", "headers", "body", "json"].contains(&&**key) {
            return Err(format!("知らないキー `{key}` があります"));
        }
    }
    let status = match map.get("status") {
        None => 200,
        Some(Value::Num(n)) if n.fract() == 0.0 && (200.0..=599.0).contains(n) => *n as u16,
        Some(other) => {
            return Err(format!("`status` は 200〜599 の整数です（{other}）"));
        }
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    match map.get("headers") {
        None => {}
        Some(Value::Map(h)) => {
            for (name, value) in h.borrow().iter() {
                let Value::Str(value) = value else {
                    return Err(format!(
                        "`headers` の値は文字列です（`{name}` は{}）",
                        value.type_name()
                    ));
                };
                valid_header(name, value)?;
                headers.push((name.to_ascii_lowercase(), value.to_string()));
            }
        }
        Some(other) => {
            return Err(format!(
                "`headers` はマップです（{}でした）",
                other.type_name()
            ));
        }
    }
    // サーバーが決めるヘッダー
    headers.retain(|(n, _)| !["content-length", "transfer-encoding", "connection"].contains(&&**n));

    let (body, default_type) = match (map.get("body"), map.get("json")) {
        (Some(_), Some(_)) => return Err("`body` と `json` は同時に使えません".into()),
        (Some(Value::Str(s)), None) => (s.as_bytes().to_vec(), Some(TEXT)),
        (Some(Value::Bytes(b)), None) => (b.to_vec(), Some("application/octet-stream")),
        (Some(other), None) => {
            return Err(format!(
                "`body` は文字列かバイト列です（{}でした）",
                other.type_name()
            ));
        }
        (None, Some(json)) => (
            to_json_string("http.serve", json, None)?.into_bytes(),
            Some("application/json"),
        ),
        (None, None) => (Vec::new(), None),
    };
    if let Some(default_type) = default_type
        && !headers.iter().any(|(n, _)| n == "content-type")
    {
        headers.push(("content-type".into(), default_type.into()));
    }
    Ok(ResponseData {
        status,
        headers,
        body,
    })
}

fn valid_header(name: &str, value: &str) -> Result<(), String> {
    let name_ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    let value_ok = value
        .bytes()
        .all(|b| b == b'\t' || (b >= 0x20 && b != 0x7f));
    if name_ok && value_ok {
        Ok(())
    } else {
        Err(format!("ヘッダー `{name}` に使えない文字があります"))
    }
}

// ---- 補助スレッド: 接続 ----

fn accept_loop(listener: &TcpListener, jobs: &Sender<Job>, max_body: usize) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let jobs = jobs.clone();
        let active = Arc::clone(&active);
        thread::spawn(move || {
            let _ = connection(stream, &jobs, max_body);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

enum ReadError {
    /// 次のリクエストが来る前に、相手が閉じた
    Closed,
    Io,
    /// このステータスで断って、接続を閉じる
    Reject(u16, &'static str),
}

impl From<io::Error> for ReadError {
    fn from(_: io::Error) -> Self {
        ReadError::Io
    }
}

const BAD: ReadError = ReadError::Reject(400, "Bad Request");

fn eof() -> ReadError {
    ReadError::Io
}

struct Parsed {
    req: RequestData,
    keep_alive: bool,
    head_only: bool,
}

fn connection(stream: TcpStream, jobs: &Sender<Job>, max_body: usize) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(IDLE_TIMEOUT))?;
    let remote = stream
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    loop {
        let parsed = match read_request(&mut reader, &mut writer, max_body, &remote) {
            Ok(parsed) => parsed,
            Err(ReadError::Closed | ReadError::Io) => return Ok(()),
            Err(ReadError::Reject(status, text)) => {
                let response = ResponseData {
                    status,
                    headers: vec![("content-type".into(), TEXT.into())],
                    body: text.as_bytes().to_vec(),
                };
                return write_response(&mut writer, &response, false, false);
            }
        };
        let (reply, answer) = mpsc::channel();
        let job = Job {
            req: parsed.req,
            reply,
        };
        if jobs.send(job).is_err() {
            return Ok(());
        }
        let Ok(response) = answer.recv() else {
            return Ok(());
        };
        write_response(&mut writer, &response, parsed.head_only, parsed.keep_alive)?;
        if !parsed.keep_alive {
            return Ok(());
        }
    }
}

/// 1 行（末尾の改行を除く）。`remaining` は、request line とヘッダーに残っている大きさ
fn read_line(
    reader: &mut BufReader<TcpStream>,
    remaining: &mut usize,
) -> Result<Option<Vec<u8>>, ReadError> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(*remaining as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if n > *remaining {
        return Err(ReadError::Reject(431, "Request Header Fields Too Large"));
    }
    *remaining -= n;
    if buf.pop() != Some(b'\n') {
        return Err(eof());
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(buf))
}

fn read_request(
    reader: &mut BufReader<TcpStream>,
    writer: &mut TcpStream,
    max_body: usize,
    remote: &str,
) -> Result<Parsed, ReadError> {
    let mut remaining = MAX_HEAD;
    // リクエストの前の空行は読み飛ばす
    let line = loop {
        match read_line(reader, &mut remaining)? {
            None => return Err(ReadError::Closed),
            Some(line) if line.is_empty() => continue,
            Some(line) => break line,
        }
    };
    let line = String::from_utf8(line).map_err(|_| BAD)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(BAD);
    };
    let http10 = match version {
        "HTTP/1.1" => false,
        "HTTP/1.0" => true,
        v if v.starts_with("HTTP/") => {
            return Err(ReadError::Reject(505, "HTTP Version Not Supported"));
        }
        _ => return Err(BAD),
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase() || b == b'-') {
        return Err(BAD);
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let Some(line) = read_line(reader, &mut remaining)? else {
            return Err(eof());
        };
        if line.is_empty() {
            break;
        }
        let line = String::from_utf8(line).map_err(|_| BAD)?;
        let (name, value) = line.split_once(':').ok_or(BAD)?;
        if valid_header(name, "").is_err() {
            return Err(BAD);
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim().to_string();
        match headers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, old)) => {
                old.push_str(", ");
                old.push_str(&value);
            }
            None => headers.push((name, value)),
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };

    let connection = header("connection").unwrap_or("").to_ascii_lowercase();
    let keep_alive = if http10 {
        connection.contains("keep-alive")
    } else {
        !connection.contains("close")
    };

    let chunked = match header("transfer-encoding") {
        None => false,
        Some(te) if te.eq_ignore_ascii_case("chunked") => true,
        Some(_) => return Err(ReadError::Reject(501, "Not Implemented")),
    };
    let length = match header("content-length") {
        None => None,
        Some(_) if chunked => return Err(BAD),
        Some(v) => {
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(BAD);
            }
            let n = v
                .parse::<usize>()
                .map_err(|_| ReadError::Reject(413, "Payload Too Large"))?;
            Some(n)
        }
    };
    if length.is_some_and(|n| n > max_body) {
        return Err(ReadError::Reject(413, "Payload Too Large"));
    }
    if header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
        && (chunked || length.is_some_and(|n| n > 0))
    {
        writer.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let body = if chunked {
        read_chunked(reader, max_body)?
    } else {
        let mut body = vec![0; length.unwrap_or(0)];
        reader.read_exact(&mut body)?;
        body
    };

    let (path, query) = split_target(target)?;
    Ok(Parsed {
        req: RequestData {
            method: method.to_string(),
            path,
            query,
            headers,
            body,
            remote: remote.to_string(),
        },
        keep_alive,
        head_only: method == "HEAD",
    })
}

fn read_chunked(reader: &mut BufReader<TcpStream>, max_body: usize) -> Result<Vec<u8>, ReadError> {
    let mut body = Vec::new();
    loop {
        let mut remaining = MAX_HEAD;
        let Some(line) = read_line(reader, &mut remaining)? else {
            return Err(eof());
        };
        let line = String::from_utf8(line).map_err(|_| BAD)?;
        let size = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| BAD)?;
        if size == 0 {
            // トレーラーは読み捨てる
            loop {
                match read_line(reader, &mut remaining)? {
                    Some(line) if !line.is_empty() => {}
                    _ => return Ok(body),
                }
            }
        }
        if body.len().saturating_add(size) > max_body {
            return Err(ReadError::Reject(413, "Payload Too Large"));
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        if &crlf != b"\r\n" {
            return Err(BAD);
        }
    }
}

type Target = (String, Vec<(String, String)>);

/// リクエストの対象を、デコード済みのパスとクエリにする。`http://host/path?q` の形も受け付ける
fn split_target(target: &str) -> Result<Target, ReadError> {
    let target = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("https://"))
        .map_or(target, |rest| rest.find('/').map_or("/", |i| &rest[i..]));
    let target = target.split('#').next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if !path.starts_with('/') && path != "*" {
        return Err(BAD);
    }
    let path = percent_decode(path, false).ok_or(BAD)?;
    let mut pairs: Vec<(String, String)> = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (
            percent_decode(k, true).ok_or(BAD)?,
            percent_decode(v, true).ok_or(BAD)?,
        );
        match pairs.iter_mut().find(|(n, _)| *n == k) {
            Some((_, old)) => *old = v,
            None => pairs.push((k, v)),
        }
    }
    Ok((path, pairs))
}

/// `%XX` を戻す。`plus` なら `+` を空白にする（クエリ）。UTF-8 にならなければ None
fn percent_decode(s: &str, plus: bool) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                let hex = std::str::from_utf8(hex).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn write_response(
    writer: &mut TcpStream,
    response: &ResponseData,
    head_only: bool,
    keep_alive: bool,
) -> io::Result<()> {
    let mut out = Vec::with_capacity(response.body.len() + 256);
    write!(
        out,
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    )?;
    for (name, value) in &response.headers {
        write!(out, "{name}: {value}\r\n")?;
    }
    write!(out, "content-length: {}\r\n", response.body.len())?;
    let connection = if keep_alive { "keep-alive" } else { "close" };
    write!(out, "connection: {connection}\r\n\r\n")?;
    if !head_only && !matches!(response.status, 204 | 304) {
        out.extend_from_slice(&response.body);
    }
    writer.write_all(&out)?;
    writer.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(target: &str) -> Target {
        match split_target(target) {
            Ok(t) => t,
            Err(_) => panic!("{target} を解析できません"),
        }
    }

    #[test]
    fn decodes_percent_and_plus() {
        assert_eq!(percent_decode("a%20b+c", false).unwrap(), "a b+c");
        assert_eq!(percent_decode("a%20b+c", true).unwrap(), "a b c");
        assert_eq!(percent_decode("%E3%81%82", false).unwrap(), "あ");
        assert!(percent_decode("%E3", false).is_none());
        assert!(percent_decode("%zz", false).is_none());
        assert!(percent_decode("%2", false).is_none());
    }

    #[test]
    fn splits_target() {
        let (path, query) = split("/a%20b?x=1&y=%E3%81%82&x=2&z");
        assert_eq!(path, "/a b");
        assert_eq!(
            query,
            [
                ("x".to_string(), "2".to_string()),
                ("y".to_string(), "あ".to_string()),
                ("z".to_string(), String::new())
            ]
        );
        assert_eq!(split("http://example.com/p?q=1").0, "/p");
        assert!(split_target("p").is_err());
    }
}
