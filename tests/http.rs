//! `std:http` のテスト。外には出ず、このテストが立てるローカルのサーバーとだけ通信する

#![cfg(feature = "http")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// テスト用の小さな HTTP サーバーを立て、ポート番号を返す。1 接続 1 リクエストで閉じる
fn start_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || handle(stream));
        }
    });
    port
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> &str {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v)
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some(Request {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn handle(mut stream: TcpStream) {
    let Some(req) = read_request(&mut stream) else {
        return;
    };
    let path = req.target.split('?').next().unwrap_or("");
    match path {
        "/hello" => respond(
            &mut stream,
            "200 OK",
            &[("X-Test", "yes"), ("X-Multi", "a"), ("X-Multi", "b")],
            b"hello",
        ),
        "/echo" => {
            let body = format!(
                "{{\"method\":\"{}\",\"target\":\"{}\",\"content_type\":\"{}\",\"token\":\"{}\",\"user_agent\":\"{}\",\"body\":\"{}\"}}",
                req.method,
                json_escape(&req.target),
                json_escape(req.header("content-type")),
                json_escape(req.header("x-token")),
                json_escape(req.header("user-agent")),
                json_escape(&req.body),
            );
            respond(&mut stream, "200 OK", &[], body.as_bytes());
        }
        "/notfound" => respond(&mut stream, "404 Not Found", &[], b"nope"),
        "/redirect" => respond(&mut stream, "302 Found", &[("Location", "/hello")], b""),
        "/loop" => respond(&mut stream, "302 Found", &[("Location", "/loop")], b""),
        "/slow" => {
            thread::sleep(Duration::from_millis(1500));
            respond(&mut stream, "200 OK", &[], b"late");
        }
        "/binary" => respond(&mut stream, "200 OK", &[], &[0xff, 0xfe, 0xfd]),
        _ => respond(&mut stream, "500 Internal Server Error", &[], b"?"),
    }
}

/// スクリプトを実行して出力を返す。`PORT` はテスト用サーバーのポートに置き換える
fn run(port: u16, script: &str) -> String {
    let src =
        format!("@http := import[\"std:http\"]\n@base := \"http://127.0.0.1:{port}\"\n{script}");
    let mut out = Vec::new();
    if let Err(diag) = dolphin::run_source(&src, &mut out) {
        panic!(
            "{}\n{}",
            diag.render("test.dol", &src),
            String::from_utf8_lossy(&out)
        );
    }
    String::from_utf8(out).unwrap()
}

fn run_err(port: u16, script: &str) -> String {
    let src =
        format!("@http := import[\"std:http\"]\n@base := \"http://127.0.0.1:{port}\"\n{script}");
    let mut out = Vec::new();
    dolphin::run_source(&src, &mut out).unwrap_err().message
}

#[test]
fn get_response_map() {
    let port = start_server();
    let out = run(
        port,
        r#"
@r := @http.get[@base + "/hello"]?
log[@r{"status"}, @r{"ok"}, @r{"body"}]
log[@r{"headers"}{"x-test"}, @r{"headers"}{"x-multi"}]
log[@r{"headers"}{"content-length"}]
"#,
    );
    assert_eq!(out, "200 true hello\nyes a, b\n5\n");
}

#[test]
fn post_json_headers_and_query() {
    let port = start_server();
    let out = run(
        port,
        r#"
@json := import["std:json"]
@r := @http.post[@base + "/echo?x=1", {"json": {"a": 1}, "headers": {"X-Token": "t1"}, "query": {"q": "日本 語", "n": 2, "ok": true}}]?
@d := @json.parse[@r{"body"}]?
log[@d{"method"}, @d{"content_type"}, @d{"token"}, @d{"body"}]
log[@d{"target"}]
log[str_contains[@d{"user_agent"}, "dolphin/"]]
"#,
    );
    assert_eq!(
        out,
        "POST application/json t1 {\"a\":1}\n/echo?x=1&q=%E6%97%A5%E6%9C%AC%20%E8%AA%9E&n=2&ok=true\ntrue\n"
    );
}

#[test]
fn body_and_request() {
    let port = start_server();
    let out = run(
        port,
        r#"
@json := import["std:json"]
@r := @http.put[@base + "/echo", {"body": "raw text", "headers": {"Content-Type": "text/plain", "User-Agent": "mine/1"}}]?
@d := @json.parse[@r{"body"}]?
log[@d{"method"}, @d{"content_type"}, @d{"body"}, @d{"user_agent"}]
@r2 := @http.request[{"url": @base + "/echo", "method": "delete"}]?
log[@json.parse[@r2{"body"}]{"method"}]
@r3 := @http.get[@base + "/echo", {"json": {"a": 1}, "headers": {"content-type": "application/vnd.x+json"}}]?
log[@json.parse[@r3{"body"}]{"content_type"}]
"#,
    );
    assert_eq!(
        out,
        "PUT text/plain raw text mine/1\nDELETE\napplication/vnd.x+json\n"
    );
}

#[test]
fn error_statuses_are_normal_responses() {
    let port = start_server();
    let out = run(
        port,
        r#"
@r := @http.get[@base + "/notfound"]?
log[@r{"status"}, @r{"ok"}, @r{"body"}]
@r2 := @http.get[@base + "/nothing-here"]?
log[@r2{"status"}, @r2{"ok"}]
"#,
    );
    assert_eq!(out, "404 false nope\n500 false\n");
}

#[test]
fn redirects() {
    let port = start_server();
    let out = run(
        port,
        r#"
log[@http.get[@base + "/redirect"]?{"body"}]
@r := @http.get[@base + "/redirect", {"redirects": 0}]?
log[@r{"status"}, @r{"headers"}{"location"}]
@loop := @http.get[@base + "/loop", {"redirects": 3}]?
log[@loop{"status"}]
"#,
    );
    assert_eq!(out, "hello\n302 /hello\n302\n");
}

#[test]
fn failures_are_error_values() {
    let port = start_server();
    // 閉じたポートへの接続
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let out = run(
        port,
        &format!(
            r#"
@slow := @http.get[@base + "/slow", {{"timeout": 0.3}}]
log[is_error[@slow], err_kind[@slow]]
@refused := @http.get["http://127.0.0.1:{closed}/"]
log[is_error[@refused], err_kind[@refused]]
@binary := @http.get[@base + "/binary"]
log[is_error[@binary], err_kind[@binary]]
@bad := @http.get["not a url"]
log[is_error[@bad], err_kind[@bad]]
@badheader := @http.get[@base + "/hello", {{"headers": {{"bad header": "x"}}}}]
log[is_error[@badheader], err_kind[@badheader]]
"#
        ),
    );
    assert_eq!(
        out,
        "true timeout\ntrue network\ntrue parse\ntrue url\ntrue request\n"
    );
}

#[test]
fn failure_can_be_propagated_with_question_mark() {
    let port = start_server();
    let out = run(
        port,
        r#"
fetch[@url] (
    @r := @http.get[@url, {"timeout": 0.3}]?
    # @r{"body"}
)
log[fetch[@base + "/hello"]]
log[err_kind[fetch[@base + "/slow"]]]
"#,
    );
    assert_eq!(out, "hello\ntimeout\n");
}

#[test]
fn option_mistakes_are_runtime_errors() {
    let port = start_server();
    let cases = [
        (
            r#"@http.get[@base, {"nope": 1}]"#,
            "http.get に知らないオプション `nope` があります",
        ),
        (
            r#"@http.get[@base, {"method": "POST"}]"#,
            "http.get では `method` を指定できません",
        ),
        (
            r#"@http.post[@base, {"body": "a", "json": 1}]"#,
            "`body` と `json` は同時に指定できません",
        ),
        (
            r#"@http.get[@base, {"timeout": 0}]"#,
            "`timeout` は 0 より大きい数です",
        ),
        (
            r#"@http.get[@base, {"redirects": 1.5}]"#,
            "`redirects` は 0 以上の整数です",
        ),
        (
            r#"@http.get[@base, {"headers": {"a": 1}}]"#,
            "`headers` の `a` は文字列です",
        ),
        (
            r#"@http.get[@base, {"query": {"a": {1}}}]"#,
            "`query` の `a` は文字列・数値・真偽値です",
        ),
        (r#"@http.get[1]"#, "http.get の 1 番目の引数は文字列です"),
        (
            r#"@http.request[{:}]"#,
            "http.request に `url` がありません",
        ),
        (
            r#"@http.get[@base, {"json": log}]"#,
            "http.get: 関数は JSON にできません",
        ),
    ];
    for (script, expected) in cases {
        let message = run_err(port, script);
        assert!(
            message.contains(expected),
            "{script}\n期待: {expected}\n実際: {message}"
        );
    }
}
