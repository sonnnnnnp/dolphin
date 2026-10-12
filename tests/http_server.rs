//! `std:http` の `serve` のテスト。`dolphin` を子プロセスで起動し、ローカルから HTTP で叩く

#![cfg(feature = "http")]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const SCRIPT: &str = r#"@http := import["std:http"]
@os := import["std:os"]
@counter := 0
@r := @http.serve[PORT, [@req] (
    @counter = @counter + 1
    @path := @req{"path"}
    if @path == "/hello" (
        # {"json": {"method": @req{"method"}, "query": @req{"query"}, "n": @counter}}
    )
    if @path == "/echo" (
        # {"status": 201, "headers": {"X-A": "b"}, "body": @req{"body"}}
    )
    if @path == "/raw" (
        # {"body": @req{"raw"}}
    )
    if @path == "/text" ( # "plain" )
    if @path == "/boom" ( # @req{"nope"} )
    if @path == "/bad" ( # 42 )
    if @path == "/bad_status" ( # {"status": 99} )
    if @path == "/exit" ( @os.exit[3] )
    # {"status": 404, "body": "not found: " + @path}
)]
log["serve failed", err_message[@r]]
"#;

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn start(name: &str) -> Server {
    let port = free_port();
    let dir = std::env::temp_dir().join("dolphin-serve-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, SCRIPT.replace("PORT", &port.to_string())).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "サーバーが起動しません");
        if let Ok(Some(status)) = server.child.try_wait() {
            panic!("サーバーが終了しました: {status}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    server
}

struct Response {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Response {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 1 つの応答を読む（`content-length` の本文まで）。読めなければ None
fn read_response(reader: &mut BufReader<TcpStream>) -> Option<Response> {
    let mut status_line = String::new();
    if reader.read_line(&mut status_line).ok()? == 0 {
        return None;
    }
    let status = status_line.split(' ').nth(1)?.parse().ok()?;
    let mut headers = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().ok()?;
        }
        headers.push_str(&line.to_ascii_lowercase());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Response {
        status,
        headers,
        body,
    })
}

fn connect(server: &Server) -> (TcpStream, BufReader<TcpStream>) {
    let stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let reader = BufReader::new(stream.try_clone().unwrap());
    (stream, reader)
}

/// `Connection: close` で 1 回だけ送る
fn send(server: &Server, raw: &[u8]) -> Option<Response> {
    let (mut stream, mut reader) = connect(server);
    stream.write_all(raw).unwrap();
    read_response(&mut reader)
}

fn get(server: &Server, target: &str) -> Response {
    let raw = format!("GET {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    send(server, raw.as_bytes()).unwrap()
}

#[test]
fn json_response_and_request_fields() {
    let server = start("json.dol");
    let r = get(&server, "/hello?a=1&b=x%20y+z&a=2");
    assert_eq!(r.status, 200);
    assert!(r.headers.contains("content-type: application/json"));
    assert!(r.headers.contains("connection: close"));
    assert_eq!(
        r.text(),
        r#"{"method":"GET","query":{"a":"2","b":"x y z"},"n":1}"#
    );
    // ハンドラの状態（@counter）は、リクエストをまたいで続く
    assert!(get(&server, "/hello").text().contains(r#""n":2"#));
}

#[test]
fn body_status_and_headers() {
    let server = start("echo.dol");
    let raw =
        "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 6\r\nConnection: close\r\n\r\nこん";
    let r = send(&server, raw.as_bytes()).unwrap();
    assert_eq!(r.status, 201);
    assert!(r.headers.contains("x-a: b"));
    assert!(
        r.headers
            .contains("content-type: text/plain; charset=utf-8")
    );
    assert_eq!(r.text(), "こん");

    // バイト列の本文はそのまま返り、`Content-Type` は octet-stream
    let raw = b"POST /raw HTTP/1.1\r\nContent-Length: 3\r\nConnection: close\r\n\r\n\xff\x00\x01";
    let r = send(&server, raw).unwrap();
    assert_eq!(r.body, [0xff, 0x00, 0x01]);
    assert!(r.headers.contains("application/octet-stream"));

    assert_eq!(get(&server, "/text").text(), "plain");
    let r = get(&server, "/nothing%20here");
    assert_eq!(r.status, 404);
    assert_eq!(r.text(), "not found: /nothing here");
}

#[test]
fn keep_alive_and_chunked() {
    let server = start("keepalive.dol");
    let (mut stream, mut reader) = connect(&server);
    stream
        .write_all(b"GET /text HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let r = read_response(&mut reader).unwrap();
    assert!(r.headers.contains("connection: keep-alive"));
    assert_eq!(r.text(), "plain");
    // 同じ接続で、chunked の本文を送る
    stream
        .write_all(
            b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
        )
        .unwrap();
    let r = read_response(&mut reader).unwrap();
    assert_eq!(r.text(), "abcde");
    // HEAD は本文なしで、長さだけ返す
    stream
        .write_all(b"HEAD /text HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut head = String::new();
    reader.read_to_string(&mut head).unwrap();
    assert!(head.contains("content-length: 5"));
    assert!(head.ends_with("\r\n\r\n"));
}

#[test]
fn handler_failures_become_500_and_the_server_continues() {
    let server = start("failures.dol");
    for path in ["/boom", "/bad", "/bad_status"] {
        let r = get(&server, path);
        assert_eq!(r.status, 500, "{path}");
        assert_eq!(r.text(), "Internal Server Error");
    }
    assert_eq!(get(&server, "/text").text(), "plain");
}

#[test]
fn malformed_and_oversized_requests() {
    let server = start("malformed.dol");
    let r = send(&server, b"NOT A REQUEST\r\n\r\n").unwrap();
    assert_eq!(r.status, 400);
    let r = send(&server, b"GET / HTTP/2.0\r\n\r\n").unwrap();
    assert_eq!(r.status, 505);
    let raw = b"POST /echo HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n";
    assert_eq!(send(&server, raw).unwrap().status, 413);
    let raw = format!("GET / HTTP/1.1\r\nX-Big: {}\r\n\r\n", "a".repeat(70 * 1024));
    assert_eq!(send(&server, raw.as_bytes()).unwrap().status, 431);
    // 壊れたリクエストのあとも動く
    assert_eq!(get(&server, "/text").text(), "plain");
}

#[test]
fn concurrent_connections_are_served_in_turn() {
    let server = start("concurrent.dol");
    let port = server.port;
    let threads: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(move || {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                stream
                    .write_all(b"GET /text HTTP/1.1\r\nConnection: close\r\n\r\n")
                    .unwrap();
                let mut reader = BufReader::new(stream);
                read_response(&mut reader).unwrap().text()
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), "plain");
    }
}

#[test]
fn os_exit_in_a_handler_stops_the_server() {
    let mut server = start("exit.dol");
    // 応答は返らず、プロセスが終了コード 3 で終わる
    let _ = send(&server, b"GET /exit HTTP/1.1\r\nConnection: close\r\n\r\n");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "終了しません");
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(3));
}

#[test]
fn bind_failure_is_an_error_value() {
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = busy.local_addr().unwrap().port();
    let dir = std::env::temp_dir().join("dolphin-serve-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.dol");
    fs::write(
        &path,
        format!(
            "@http := import[\"std:http\"]\n@r := @http.serve[{port}, [@req] ( # \"x\" )]\nlog[err_kind[@r]]\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "network");
}
