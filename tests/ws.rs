//! WebSocket のテスト。`dolphin` を子プロセスのサーバーとして起動し、生のソケットと、
//! `std:ws` のクライアント（もう 1 つの `dolphin`）の両方で叩く

#![cfg(feature = "http")]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use dolphin::stdlib::ws_proto::{
    self, Message, OP_BINARY, OP_CLOSE, OP_PING, OP_TEXT, accept_key, close_payload, read_message,
    write_frame,
};

const SERVER: &str = r#"@http := import["std:http"]
@ws := import["std:ws"]
@http.serve[PORT, [@req] (
    if @req{"path"} == "/echo" (
        # {"websocket": {
            "on_open": [@c] ( @ws.send[@c, "welcome"] ),
            "on_message": [@c, @m] (
                if @m == "close" (
                    @ws.close[@c, 4000, "bye"]
                ) else (
                    @ws.send[@c, @m]
                )
            ),
            "on_close": [@c, @code] ( log["closed @code"] )
        }}
    )
    if @req{"path"} == "/boom" (
        # {"websocket": {
            "on_message": [@c, @m] ( # @m{"nope"} )
        }}
    )
    # "plain"
)]
"#;

struct Server {
    child: Child,
    port: u16,
    /// サーバーの標準出力の行
    lines: Receiver<String>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(name: &str) -> Server {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = std::env::temp_dir().join("dolphin-ws-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, SERVER.replace("PORT", &port.to_string())).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let server = Server { child, port, lines };
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "サーバーが起動しません");
        thread::sleep(Duration::from_millis(50));
    }
    server
}

impl Server {
    fn next_line(&self) -> String {
        self.lines
            .recv_timeout(Duration::from_secs(10))
            .expect("サーバーの出力が来ません")
    }
}

/// ハンドシェイク済みの生のクライアント
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

fn raw_connect(server: &Server, path: &str) -> Client {
    let stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    write!(
        writer,
        "GET {path} HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .unwrap();
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    assert!(status.starts_with("HTTP/1.1 101"), "{status}");
    let mut accept = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line
            .to_ascii_lowercase()
            .strip_prefix("sec-websocket-accept:")
        {
            accept = v.trim().to_string();
        }
    }
    assert_eq!(accept.to_lowercase(), accept_key(key).to_lowercase());
    Client { reader, writer }
}

impl Client {
    fn send(&mut self, opcode: u8, payload: &[u8]) {
        write_frame(&mut self.writer, opcode, payload, Some([1, 2, 3, 4])).unwrap();
    }

    fn recv(&mut self) -> Message {
        read_message(&mut self.reader, false, |_| Ok(())).expect("メッセージを読めません")
    }
}

#[test]
fn echo_binary_ping_and_close() {
    let server = start("echo.dol");
    let mut c = raw_connect(&server, "/echo");
    assert_eq!(c.recv(), Message::Text("welcome".into()));

    c.send(OP_TEXT, "こんにちは".as_bytes());
    assert_eq!(c.recv(), Message::Text("こんにちは".into()));
    c.send(OP_BINARY, &[0, 255, 7]);
    assert_eq!(c.recv(), Message::Binary(vec![0, 255, 7]));

    // 大きなメッセージ（拡張長）
    let big = "x".repeat(70_000);
    c.send(OP_TEXT, big.as_bytes());
    assert_eq!(c.recv(), Message::Text(big));

    // ping には pong が返る（`read_message` は pong を読み捨てるので、フレームで見る）
    c.send(OP_PING, b"are you there");
    let pong = ws_proto::read_frame(&mut c.reader, false).unwrap();
    assert_eq!(
        (pong.opcode, pong.payload.as_slice()),
        (10, &b"are you there"[..])
    );

    // 断片化したメッセージ
    let mut frame = Vec::new();
    write_frame(&mut frame, OP_TEXT, b"frag", Some([9, 9, 9, 9])).unwrap();
    frame[0] &= 0x7F; // fin を落とす
    c.writer.write_all(&frame).unwrap();
    let mut rest = Vec::new();
    write_frame(&mut rest, 0, b"mented", Some([5, 6, 7, 8])).unwrap();
    c.writer.write_all(&rest).unwrap();
    assert_eq!(c.recv(), Message::Text("fragmented".into()));

    // こちらから閉じる
    c.send(OP_CLOSE, &close_payload(1000, "done"));
    assert_eq!(c.recv(), Message::Close(Some((1000, String::new()))));
    assert_eq!(server.next_line(), "closed 1000");
}

#[test]
fn server_closes_with_code() {
    let server = start("close.dol");
    let mut c = raw_connect(&server, "/echo");
    assert_eq!(c.recv(), Message::Text("welcome".into()));
    c.send(OP_TEXT, b"close");
    assert_eq!(c.recv(), Message::Close(Some((4000, "bye".into()))));
    c.send(OP_CLOSE, &close_payload(4000, ""));
    assert_eq!(server.next_line(), "closed 4000");
}

#[test]
fn protocol_violations_are_closed() {
    let server = start("violation.dol");
    // マスクなしのフレームは 1002
    let mut c = raw_connect(&server, "/echo");
    assert_eq!(c.recv(), Message::Text("welcome".into()));
    write_frame(&mut c.writer, OP_TEXT, b"unmasked", None).unwrap();
    assert!(matches!(c.recv(), Message::Close(Some((1002, _)))));
    assert_eq!(server.next_line(), "closed 1002");

    // テキストが UTF-8 でなければ 1007
    let mut c = raw_connect(&server, "/echo");
    assert_eq!(c.recv(), Message::Text("welcome".into()));
    c.send(OP_TEXT, &[0xFF, 0xFE]);
    assert!(matches!(c.recv(), Message::Close(Some((1007, _)))));
    assert_eq!(server.next_line(), "closed 1007");

    // 切断（close フレームなし）は 1006
    let mut c = raw_connect(&server, "/echo");
    assert_eq!(c.recv(), Message::Text("welcome".into()));
    drop(c);
    assert_eq!(server.next_line(), "closed 1006");
}

#[test]
fn non_websocket_request_gets_400_and_callback_errors_continue() {
    let server = start("bad.dol");
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .write_all(b"GET /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");

    // コールバックのエラーでは、サーバーも接続も続く
    let mut c = raw_connect(&server, "/boom");
    c.send(OP_TEXT, b"x");
    c.send(OP_TEXT, b"y");
    c.send(OP_CLOSE, &close_payload(1000, ""));
    assert!(matches!(c.recv(), Message::Close(_)));
    // 普通の HTTP も動く
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .write_all(b"GET /other HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("plain"), "{response}");
}

#[test]
fn dolphin_client_talks_to_dolphin_server() {
    let server = start("peer.dol");
    let dir = std::env::temp_dir().join("dolphin-ws-tests");
    let path = dir.join("client.dol");
    fs::write(
        &path,
        format!(
            r#"@ws := import["std:ws"]
@c := @ws.connect["ws://127.0.0.1:{port}/echo?x=1", {{"headers": {{"X-Test": "1"}}}}]
log[type_of[@c]]
log[@ws.recv[@c]]
@ws.send[@c, "hello"]
log[@ws.recv[@c]]
@ws.send[@c, str_to_bytes["ab"]]
log[@ws.recv[@c]]
@ws.set_timeout[@c, 0.2]
log[err_kind[@ws.recv[@c]]]
@ws.set_timeout[@c, nil]
@ws.send[@c, "close"]
log[@ws.recv[@c]]
log[err_kind[@ws.recv[@c]]]
log[err_kind[@ws.send[@c, "late"]]]
@ws.close[@c]
@d := @ws.connect["ws://127.0.0.1:{port}/echo"]
@ws.close[@d, 1001, "going away"]
@ws.close[@d]
log[is_error[@ws.connect["ws://127.0.0.1:{port}/plain-not-ws"]]]
log[err_kind[@ws.connect["wss://127.0.0.1/"]]]
"#,
            port = server.port
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "handle\nwelcome\nhello\n<bytes 6162>\ntimeout\nnil\nclosed\nclosed\ntrue\nnetwork\n",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // サーバー側のログ（1 本目はサーバーが 4000 で閉じ、2 本目はクライアントが 1001 で閉じた）
    let mut seen = vec![server.next_line(), server.next_line()];
    seen.sort();
    assert_eq!(seen, ["closed 1001", "closed 4000"]);
}
