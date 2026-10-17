//! `std:ws`: connect, send, recv, close, set_timeout（design.md 5.22）
//!
//! クライアント側は標準ライブラリだけで、ブロッキング。サーバー側の接続（`http.serve` の
//! `websocket`）の `send` / `close` も、ここで受ける

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::encoding::base64_encode;
use super::net::{closed, failure, seconds};
use super::ws_proto::{
    Message, OP_BINARY, OP_CLOSE, OP_PING, OP_PONG, OP_TEXT, ProtoError, accept_key, close_payload,
    read_message, read_message_or_control, write_frame,
};
use super::{error_value, expect_args, int_arg, num_arg, str_arg, type_error};
use crate::runtime::data::Data;
use crate::runtime::events::{Event, StreamEvent};
use crate::runtime::handle::{Handle, HandleKind, WsConn, WsOut};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

const DEFAULT_TIMEOUT: f64 = 30.0;
/// ハンドシェイクの応答の大きさの上限
const MAX_HEAD: usize = 64 * 1024;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:ws",
        &[
            ("ws.connect", connect),
            ("ws.connect_async", connect_async),
            ("ws.send", send),
            ("ws.recv", recv),
            ("ws.close", close),
            ("ws.set_timeout", set_timeout),
        ],
    );
}

// ---- 乱数（マスクキーとハンドシェイクのキー。暗号の強さは要らない） ----

fn random_bytes<const N: usize>() -> [u8; N] {
    use std::cell::Cell;
    static SEED_ANCHOR: u8 = 0;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
                ^ ((&raw const SEED_ANCHOR) as u64),
        );
    }
    let mut out = [0u8; N];
    STATE.with(|state| {
        for chunk in out.chunks_mut(8) {
            // splitmix64
            let mut z = state.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
            state.set(z);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes()[..chunk.len()]);
        }
    });
    out
}

// ---- 接続 ----

#[derive(Debug)]
struct Url {
    /// `wss://`（TLS）か
    tls: bool,
    host: String,
    port: u16,
    /// パスとクエリ（`/` で始まる）
    target: String,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("ws://") {
        (false, rest)
    } else if let Some(rest) = url.strip_prefix("wss://") {
        (true, rest)
    } else {
        return Err("URL は ws:// か wss:// で始まる必要があります".into());
    };
    let (authority, target) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !authority.ends_with(']') => (
            host,
            port.parse::<u16>()
                .map_err(|_| format!("ポート `{port}` が正しくありません"))?,
        ),
        _ => (authority, if tls { 443 } else { 80 }),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return Err("URL にホストがありません".into());
    }
    Ok(Url {
        tls,
        host: host.to_string(),
        port,
        target,
    })
}

fn connect(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "ws.connect の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let url = str_arg("ws.connect", args, 0)?;
    let opts = match args.get(1) {
        Some(options) => connect_options("ws.connect", options, 1, false)?,
        None => ConnectOptions::default(),
    };
    let (timeout, headers) = (opts.timeout, opts.headers);
    let timeout = seconds("ws.connect", timeout)?;
    let url = match parse_url(url) {
        Ok(url) => url,
        Err(message) => return Ok(error_value("network", message)),
    };
    Ok(match open(&url, &headers, timeout) {
        Ok((reader, writer)) => Value::Handle(Handle::new(HandleKind::WsClient { reader, writer })),
        Err(e) => e,
    })
}

struct ConnectOptions {
    timeout: f64,
    headers: Vec<(String, String)>,
    /// こちらから ping を送る間隔（秒）。`connect_async` だけ
    ping_interval: Option<f64>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            headers: Vec::new(),
            ping_interval: None,
        }
    }
}

/// `ws.connect` / `ws.connect_async` の `options`（`args[i]`）を読む
fn connect_options(
    name: &str,
    options: &Value,
    i: usize,
    allow_ping: bool,
) -> Result<ConnectOptions, String> {
    let Value::Map(options) = options else {
        return Err(type_error(name, i, "マップ", options));
    };
    let mut opts = ConnectOptions::default();
    for (key, value) in options.borrow().iter() {
        match (&**key, value) {
            ("timeout", Value::Num(n)) => opts.timeout = *n,
            ("ping_interval", Value::Num(n)) if allow_ping => opts.ping_interval = Some(*n),
            ("headers", Value::Map(h)) => {
                for (header, value) in h.borrow().iter() {
                    let Value::Str(value) = value else {
                        return Err(format!(
                            "{name} の `headers` の値は文字列です（`{header}` は{}）",
                            value.type_name()
                        ));
                    };
                    if header.bytes().any(|b| b <= b' ' || b == b':' || b == 0x7f)
                        || value.bytes().any(|b| b == b'\r' || b == b'\n')
                    {
                        return Err(format!(
                            "{name} のヘッダー `{header}` に使えない文字があります"
                        ));
                    }
                    opts.headers.push((header.to_string(), value.to_string()));
                }
            }
            ("timeout" | "headers", other) => {
                return Err(format!(
                    "{name} のオプション `{key}` の型が違います（{}が渡されました）",
                    other.type_name()
                ));
            }
            ("ping_interval", other) if allow_ping => {
                return Err(format!(
                    "{name} のオプション `{key}` の型が違います（{}が渡されました）",
                    other.type_name()
                ));
            }
            _ => return Err(format!("{name} のオプション `{key}` は知りません")),
        }
    }
    Ok(opts)
}

/// TCP の接続とハンドシェイク（読む側と書く側）。失敗はエラー値
fn open(
    url: &Url,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<(BufReader<WsConn>, WsConn), Value> {
    let io_error = |what: &str, e: &io::Error| failure(what, e);
    let addrs = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| io_error(&format!("{} を引けません", url.host), &e))?;
    let mut stream = None;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let Some(stream) = stream else {
        return Err(match last {
            Some(e) => io_error(&format!("{}:{} に接続できません", url.host, url.port), &e),
            None => error_value(
                "network",
                format!("{} のアドレスが見つかりません", url.host),
            ),
        });
    };
    let setup = |e: io::Error| io_error("接続を準備できません", &e);
    stream.set_nodelay(true).map_err(setup)?;
    // ハンドシェイクの間だけ、読み書きに時間切れをつける（TLS のハンドシェイクもこの間に行う）
    stream.set_read_timeout(Some(timeout)).map_err(setup)?;
    stream.set_write_timeout(Some(timeout)).map_err(setup)?;
    let stream = if url.tls {
        tls_wrap(&url.host, stream)?
    } else {
        WsConn::Plain(stream)
    };

    let key = base64_encode(&random_bytes::<16>(), false);
    // `Host` は、ポートが既定なら付けない。IPv6 は `[::1]` の形
    let host = if url.host.contains(':') {
        format!("[{}]", url.host)
    } else {
        url.host.clone()
    };
    let host = if url.port == if url.tls { 443 } else { 80 } {
        host
    } else {
        format!("{host}:{}", url.port)
    };
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n",
        url.target
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    let mut writer = stream.try_clone().map_err(setup)?;
    writer
        .write_all(request.as_bytes())
        .map_err(|e| io_error("ハンドシェイクを送れません", &e))?;
    let mut reader = BufReader::new(stream);

    let read_error = |e: &io::Error| io_error("ハンドシェイクの応答を読めません", e);
    let mut remaining = MAX_HEAD;
    let mut status_line = String::new();
    read_head_line(&mut reader, &mut status_line, &mut remaining).map_err(|e| read_error(&e))?;
    let status = status_line.split(' ').nth(1).unwrap_or("");
    let mut accept = None;
    loop {
        let mut line = String::new();
        read_head_line(&mut reader, &mut line, &mut remaining).map_err(|e| read_error(&e))?;
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("sec-websocket-accept")
        {
            accept = Some(value.trim().to_string());
        }
    }
    if status != "101" {
        return Err(error_value(
            "network",
            format!("WebSocket にできませんでした（{}）", status_line.trim()),
        ));
    }
    if accept.as_deref() != Some(accept_key(&key).as_str()) {
        return Err(error_value(
            "network",
            "Sec-WebSocket-Accept が正しくありません",
        ));
    }
    // ハンドシェイクのあとは、時間切れなし（`set_timeout` で決める）
    let reset = |e: io::Error| io_error("接続を準備できません", &e);
    reader.get_ref().set_read_timeout(None).map_err(reset)?;
    writer.set_write_timeout(None).map_err(reset)?;
    Ok((reader, writer))
}

/// TCP の接続を TLS にする（証明書とホスト名は必ず検証する。design.md 5.35）。
/// TLS のハンドシェイクは、最初の読み書きで行われる
#[cfg(feature = "tls")]
fn tls_wrap(host: &str, stream: TcpStream) -> Result<WsConn, Value> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, OnceLock};

    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    let config = CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring は TLS 1.2 と 1.3 に対応している")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone();
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| {
        error_value(
            "network",
            format!("`{host}` は TLS のホスト名として使えません"),
        )
    })?;
    let conn = rustls::ClientConnection::new(config, name)
        .map_err(|e| error_value("network", format!("TLS を始められません: {e}")))?;
    Ok(WsConn::Tls(Rc::new(RefCell::new(
        rustls::StreamOwned::new(conn, stream),
    ))))
}

#[cfg(not(feature = "tls"))]
fn tls_wrap(_: &str, _: TcpStream) -> Result<WsConn, Value> {
    Err(error_value(
        "network",
        "この版には wss:// が含まれていません（`tls` feature を付けてビルドしてください）",
    ))
}

/// ヘッダーの 1 行（末尾の改行を除く）を `line` に読む。大きさを `remaining` で制限する
fn read_head_line(
    reader: &mut BufReader<WsConn>,
    line: &mut String,
    remaining: &mut usize,
) -> io::Result<()> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(*remaining as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    if n > *remaining {
        return Err(io::Error::other("ハンドシェイクの応答が大きすぎます"));
    }
    *remaining -= n;
    *line = String::from_utf8_lossy(&buf)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    Ok(())
}

// ---- コールバック版（design.md 5.37） ----

const HANDLER_KEYS: [&str; 4] = ["on_open", "on_message", "on_close", "on_error"];
/// 補助スレッドが、送信の依頼と停止の合図を見に戻る間隔
const POLL: Duration = Duration::from_millis(20);
/// こちらから close を送ったあと、相手の close を待つ時間
const CLOSE_WAIT: Duration = Duration::from_secs(3);

fn connect_async(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "ws.connect_async";
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "{NAME} の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let url = str_arg(NAME, args, 0)?.to_string();
    let handlers = &args[args.len() - 1];
    let Value::Map(map) = handlers else {
        return Err(format!(
            "{NAME} の最後の引数はコールバックのマップです（{}が渡されました）",
            handlers.type_name()
        ));
    };
    for (key, value) in map.borrow().iter() {
        if !HANDLER_KEYS.contains(&&**key) {
            return Err(format!(
                "{NAME} のコールバックに知らないキー `{key}` があります"
            ));
        }
        if !matches!(value, Value::Func(_) | Value::Native(_)) {
            return Err(format!(
                "{NAME} の `{key}` は関数です（{}でした）",
                value.type_name()
            ));
        }
    }
    let opts = if args.len() == 3 {
        connect_options(NAME, &args[1], 1, true)?
    } else {
        ConnectOptions::default()
    };
    let timeout = seconds(NAME, opts.timeout)?;
    let ping = opts.ping_interval.map(|n| seconds(NAME, n)).transpose()?;

    let stop = Arc::new(AtomicBool::new(false));
    let (out_tx, out_rx) = mpsc::channel::<WsOut>();
    let conn = Value::Handle(Handle::new(HandleKind::WsServer { tx: out_tx }));
    let id = interp
        .events
        .register_stream(handlers.clone(), conn, Arc::clone(&stop))?;
    let job = ClientJob {
        url: parse_url(&url),
        headers: opts.headers,
        timeout,
        ping,
        id,
        events: interp.events.sender(),
        out_rx,
        stop,
    };
    thread::spawn(move || client_thread(job));
    Ok(Value::Num(id as f64))
}

struct ClientJob {
    url: Result<Url, String>,
    headers: Vec<(String, String)>,
    timeout: Duration,
    ping: Option<Duration>,
    id: u64,
    events: Sender<Event>,
    out_rx: Receiver<WsOut>,
    stop: Arc<AtomicBool>,
}

/// 補助スレッド: 接続して、送信の依頼を書き、届いたものをイベントとして送る
fn client_thread(job: ClientJob) {
    let ClientJob {
        url,
        headers,
        timeout,
        ping,
        id,
        events,
        out_rx,
        stop,
    } = job;
    let emit = |event: StreamEvent| events.send(Event::Stream { id, event }).is_ok();
    let fail = |value: Value| {
        // 止められたあとの失敗は、誰も待っていない
        if !stop.load(Ordering::SeqCst) {
            emit(StreamEvent::Error(Data::from_value(&value)));
        }
    };
    let url = match url {
        Ok(url) => url,
        Err(message) => return fail(error_value("network", message)),
    };
    let (mut reader, mut writer) = match open(&url, &headers, timeout) {
        Ok(streams) => streams,
        Err(e) => return fail(e),
    };
    if stop.load(Ordering::SeqCst) {
        close_quietly(&mut writer);
        return;
    }
    if !emit(StreamEvent::Open) {
        return;
    }
    if let Some(result) = run_client(&mut reader, &mut writer, &out_rx, &stop, ping, &emit) {
        let (code, reason) = result;
        emit(StreamEvent::Close(code, reason));
    }
}

fn mask() -> Option<[u8; 4]> {
    Some(random_bytes::<4>())
}

fn close_quietly(writer: &mut WsConn) {
    let payload = close_payload(1000, "");
    let _ = write_frame(writer, OP_CLOSE, &payload, mask());
    writer.close_notify();
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// 接続の面倒を見る。閉じたら `Some((終了コード, 理由))`。止められて静かに閉じたら `None`
fn run_client(
    reader: &mut BufReader<WsConn>,
    writer: &mut WsConn,
    out_rx: &Receiver<WsOut>,
    stop: &AtomicBool,
    ping: Option<Duration>,
    emit: &dyn Fn(StreamEvent) -> bool,
) -> Option<(u16, String)> {
    let lost = |e: &dyn std::fmt::Display| Some((1006, format!("接続が切れました: {e}")));
    if let Err(e) = reader.get_ref().set_read_timeout(Some(POLL)) {
        return lost(&e);
    }
    // こちらから close を送ったとき: （終了コード, 理由, 送った時刻）
    let mut closing: Option<(u16, String, Instant)> = None;
    let mut last_ping = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            close_quietly(writer);
            return None;
        }
        // 送信の依頼
        while let Ok(out) = out_rx.try_recv() {
            let written = match out {
                _ if closing.is_some() => Ok(()),
                WsOut::Text(s) => write_frame(writer, OP_TEXT, s.as_bytes(), mask()),
                WsOut::Binary(b) => write_frame(writer, OP_BINARY, &b, mask()),
                WsOut::Pong(p) => write_frame(writer, OP_PONG, &p, mask()),
                WsOut::Close(code, reason) => {
                    let payload = close_payload(code, &reason);
                    closing = Some((code, reason, Instant::now()));
                    write_frame(writer, OP_CLOSE, &payload, mask())
                }
            };
            if let Err(e) = written {
                return lost(&e);
            }
        }
        if let Some((code, reason, since)) = &closing
            && since.elapsed() > CLOSE_WAIT
        {
            writer.close_notify();
            return Some((*code, reason.clone()));
        }
        if let Some(interval) = ping
            && closing.is_none()
            && last_ping.elapsed() >= interval
        {
            last_ping = Instant::now();
            if let Err(e) = write_frame(writer, OP_PING, b"", mask()) {
                return lost(&e);
            }
        }
        // 届いたもの。1 バイトでも来ていれば、そのメッセージは最後まで読む
        match reader.fill_buf() {
            Ok([]) => return lost(&"相手が接続を閉じました"),
            Ok(_) => {}
            Err(e) if is_timeout(&e) => continue,
            Err(e) => return lost(&e),
        }
        let _ = reader.get_ref().set_read_timeout(None);
        let result = read_message_or_control(reader, false, |payload| {
            write_frame(writer, OP_PONG, payload, mask())
        });
        if let Err(e) = reader.get_ref().set_read_timeout(Some(POLL)) {
            return lost(&e);
        }
        match result {
            // ping / pong だけだったので、送信の依頼を見に戻る
            Ok(None) => {}
            Ok(Some(Message::Text(s))) => {
                // 送り先（イベントループ）がなくなっていれば、静かに終わる
                if !emit(StreamEvent::Message(Data::Str(s))) {
                    return None;
                }
            }
            Ok(Some(Message::Binary(b))) => {
                if !emit(StreamEvent::Message(Data::Bytes(b))) {
                    return None;
                }
            }
            Ok(Some(Message::Close(code))) => {
                let (code, reason) = code.unwrap_or((1005, String::new()));
                if closing.is_none() {
                    let _ = write_frame(writer, OP_CLOSE, &close_payload(code, ""), mask());
                    writer.close_notify();
                    return Some((code, reason));
                }
                writer.close_notify();
                let (sent, sent_reason, _) = closing.take().expect("closing は Some");
                return Some((sent, sent_reason));
            }
            Err(ProtoError::Io(e)) => return lost(&e),
            Err(e) => {
                let reason = match &e {
                    ProtoError::Protocol(why) => (*why).to_string(),
                    ProtoError::BadUtf8 => "テキストが UTF-8 ではありません".to_string(),
                    _ => "メッセージが 16 MB を超えました".to_string(),
                };
                let code = e.close_code();
                let _ = write_frame(writer, OP_CLOSE, &close_payload(code, ""), mask());
                writer.close_notify();
                return Some((code, reason));
            }
        }
    }
}

// ---- 送受信 ----

fn handle_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Handle, String> {
    match &args[i] {
        Value::Handle(h) => Ok(h),
        other => Err(type_error(name, i, "ハンドル", other)),
    }
}

fn not_ws(name: &str) -> String {
    format!("{name} に渡せるのは WebSocket のハンドルです（TCP のハンドルでした）")
}

fn send(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ws.send", args, 2)?;
    let handle = handle_arg("ws.send", args, 0)?;
    let (opcode, payload) = match &args[1] {
        Value::Str(s) => (OP_TEXT, s.as_bytes().to_vec()),
        Value::Bytes(b) => (OP_BINARY, b.to_vec()),
        other => return Err(type_error("ws.send", 1, "文字列かバイト列", other)),
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::WsClient { writer, .. } => Ok(
            match write_frame(writer, opcode, &payload, Some(random_bytes::<4>())) {
                Ok(()) => Value::Nil,
                Err(e) => failure("送信に失敗しました", &e),
            },
        ),
        HandleKind::WsServer { tx } => {
            let out = if opcode == OP_TEXT {
                WsOut::Text(String::from_utf8(payload).expect("文字列から作った"))
            } else {
                WsOut::Binary(payload)
            };
            // 受け取る側（補助スレッド）が終わっていれば、接続は閉じている
            Ok(match tx.send(out) {
                Ok(()) => Value::Nil,
                Err(_) => closed(),
            })
        }
        HandleKind::Closed => Ok(closed()),
        _ => Err(not_ws("ws.send")),
    }
}

fn recv(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ws.recv", args, 1)?;
    let handle = handle_arg("ws.recv", args, 0)?;
    let mut inner = handle.inner.borrow_mut();
    let result = match &mut *inner {
        HandleKind::WsClient { reader, writer } => read_message(reader, false, |payload| {
            write_frame(writer, OP_PONG, payload, Some(random_bytes::<4>()))
        }),
        HandleKind::WsServer { .. } => {
            return Err(
                "コールバック式の接続（`http.serve` の `websocket` と `ws.connect_async`）は ws.recv できません（`on_message` で受け取ります）"
                    .into(),
            );
        }
        HandleKind::Closed => return Ok(closed()),
        _ => return Err(not_ws("ws.recv")),
    };
    match result {
        Ok(Message::Text(s)) => Ok(Value::Str(s.into())),
        Ok(Message::Binary(b)) => Ok(Value::bytes(b)),
        Ok(Message::Close(code)) => {
            // 相手の close に close を返して、閉じる
            if let HandleKind::WsClient { writer, .. } = &mut *inner {
                let echo = code.map_or(Vec::new(), |(c, _)| close_payload(c, ""));
                let _ = write_frame(writer, OP_CLOSE, &echo, Some(random_bytes::<4>()));
            }
            *inner = HandleKind::Closed;
            Ok(Value::Nil)
        }
        Err(e) => {
            let value = match &e {
                ProtoError::Io(io) => failure("受信に失敗しました", io),
                ProtoError::Protocol(why) => error_value("protocol", *why),
                ProtoError::BadUtf8 => error_value("parse", "テキストが UTF-8 ではありません"),
                ProtoError::TooLarge => error_value("too_large", "メッセージが 16 MB を超えました"),
            };
            // 通信の失敗（時間切れを含む）のあとも、同じ接続で続けられる。
            // 規約違反は、相手に伝えて閉じる
            if !matches!(e, ProtoError::Io(_)) {
                if let HandleKind::WsClient { writer, .. } = &mut *inner {
                    let payload = close_payload(e.close_code(), "");
                    let _ = write_frame(writer, OP_CLOSE, &payload, Some(random_bytes::<4>()));
                }
                *inner = HandleKind::Closed;
            }
            Ok(value)
        }
    }
}

fn close(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !matches!(args.len(), 1 | 3) {
        return Err(format!(
            "ws.close の引数は 1 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let handle = handle_arg("ws.close", args, 0)?;
    let (code, reason) = if args.len() == 3 {
        let code = int_arg("ws.close", args, 1)?;
        let code = u16::try_from(code)
            .ok()
            .filter(|c| (1000..5000).contains(c) && !matches!(c, 1004..=1006 | 1015))
            .ok_or_else(|| format!("ws.close の終了コードは 1000〜4999 です（{code}）"))?;
        (code, str_arg("ws.close", args, 2)?.to_string())
    } else {
        (1000, String::new())
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::WsClient { writer, .. } => {
            let payload = close_payload(code, &reason);
            // 送れなくても、閉じることに変わりはない
            let _ = write_frame(writer, OP_CLOSE, &payload, Some(random_bytes::<4>()));
            writer.close_notify();
        }
        HandleKind::WsServer { tx } => {
            let _ = tx.send(WsOut::Close(code, reason));
        }
        HandleKind::Closed => return Ok(Value::Nil),
        _ => return Err(not_ws("ws.close")),
    }
    *inner = HandleKind::Closed;
    Ok(Value::Nil)
}

fn set_timeout(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ws.set_timeout", args, 2)?;
    let handle = handle_arg("ws.set_timeout", args, 0)?;
    let timeout = match &args[1] {
        Value::Nil => None,
        Value::Num(_) => Some(seconds(
            "ws.set_timeout",
            num_arg("ws.set_timeout", args, 1)?,
        )?),
        other => return Err(type_error("ws.set_timeout", 1, "数値か nil", other)),
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::WsClient { reader, writer } => {
            let result = reader
                .get_ref()
                .set_read_timeout(timeout)
                .and_then(|()| writer.set_write_timeout(timeout));
            Ok(match result {
                Ok(()) => Value::Nil,
                Err(e) => failure("時間切れを設定できません", &e),
            })
        }
        HandleKind::WsServer { .. } => {
            Err("コールバック式の接続（`http.serve` の `websocket` と `ws.connect_async`）には ws.set_timeout を使えません".into())
        }
        HandleKind::Closed => Ok(closed()),
        _ => Err(not_ws("ws.set_timeout")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("ws://example.com/chat?room=1").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.target.as_str()),
            ("example.com", 80, "/chat?room=1")
        );
        let u = parse_url("ws://localhost:8080").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.target.as_str()),
            ("localhost", 8080, "/")
        );
        let u = parse_url("ws://[::1]:9/x").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.target.as_str()),
            ("::1", 9, "/x")
        );
        let u = parse_url("ws://h?q=1").unwrap();
        assert_eq!(u.target, "/?q=1");
        let u = parse_url("wss://h/x").unwrap();
        assert_eq!((u.tls, u.port, u.target.as_str()), (true, 443, "/x"));
        let u = parse_url("wss://h:8443").unwrap();
        assert_eq!((u.tls, u.port), (true, 8443));
        assert!(!parse_url("ws://h").unwrap().tls);
        assert!(parse_url("http://h/").is_err());
        assert!(parse_url("ws://h:99999/").is_err());
        assert!(parse_url("ws:///x").is_err());
    }

    #[test]
    fn random_differs() {
        assert_ne!(random_bytes::<16>(), random_bytes::<16>());
    }
}
