//! 非同期の関数（`http.get_async` `os.run_async`）のテスト。外には出ず、このテストが立てる
//! ローカルのサーバーとだけ通信する

#![cfg(feature = "http")]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// 小さな HTTP サーバー。パスが `/slow/<ミリ秒>` なら、その時間だけ待ってから応答する
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

fn handle(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    // ヘッダーを読み、本文があれば（読み残すと接続がリセットされるので）読み捨てる
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let path = request_line.split(' ').nth(1).unwrap_or("/").to_string();
    if let Some(ms) = path.strip_prefix("/slow/") {
        thread::sleep(Duration::from_millis(ms.parse().unwrap()));
    }
    let body = format!("reply {path}");
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn run_script(name: &str, source: &str) -> (String, Duration) {
    let dir = std::env::temp_dir().join("dolphin-async-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, source).unwrap();
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    child.wait().unwrap();
    assert!(stderr.is_empty(), "{stderr}");
    (stdout.replace("\r\n", "\n"), started.elapsed())
}

#[test]
fn requests_run_concurrently_and_callbacks_run_in_order_of_completion() {
    let port = start_server();
    let (out, elapsed) = run_script(
        "get.dol",
        &format!(
            r#"@http := import["std:http"]
@base := "http://127.0.0.1:{port}"
@http.get_async["@base/slow/600", [@r] ( log["A", @r{{"status"}}, @r{{"body"}}] )]
@http.get_async["@base/slow/50", [@r] ( log["B", @r{{"body"}}] )]
@http.get_async["@base/slow/300", {{"headers": {{"x": "1"}}}}, [@r] ( log["C", @r{{"body"}}] )]
@http.request_async[{{"url": "@base/fast", "method": "GET"}}, [@r] ( log["D", @r{{"ok"}}] )]
@http.post_async["@base/p", {{"body": "x"}}, [@r] ( log["E", @r{{"body"}}] )]
// 失敗はコールバックにエラー値で届く
@http.get_async["http://127.0.0.1:1/", {{"timeout": 2}}, [@r] ( log["F", is_error[@r]] )]
@id := @http.get_async["@base/slow/10", [@r] ( log["G", @r{{"status"}}] )]
log["main", type_of[@id]]
"#
        ),
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "main number", "{out}");
    // 完了した順（50 ms → 300 ms → 600 ms）。速いもの（D, E, F）は B より前でもよいので、
    // A・B・C の相対順だけを確かめる
    let pos = |prefix: &str| lines.iter().position(|l| l.starts_with(prefix)).unwrap();
    assert!(pos("B") < pos("C") && pos("C") < pos("A"), "{out}");
    assert!(out.contains("A 200 reply /slow/600"), "{out}");
    assert!(out.contains("D true"), "{out}");
    assert!(out.contains("E reply /p"), "{out}");
    assert!(out.contains("F true"), "{out}");
    // 600 ms のものを待ちながら、ほかは同時に進む（順番に待てば 600 + 300 + 50 ms 以上かかる）
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[test]
fn cancelled_requests_are_discarded() {
    let port = start_server();
    let (out, _) = run_script(
        "cancel.dol",
        &format!(
            r#"@http := import["std:http"]
@async := import["std:async"]
@id := @http.get_async["http://127.0.0.1:{port}/slow/100", [@r] ( log["出ない"] )]
@async.cancel[@id]
@http.get_async["http://127.0.0.1:{port}/slow/200", [@r] ( log["届く"] )]
"#
        ),
    );
    assert_eq!(out, "届く\n");
}

#[test]
fn run_async_runs_a_command() {
    let (out, _) = run_script(
        "run.dol",
        r#"@os := import["std:os"]
@os.run_async[@os.exe[], {"--nope"}, [@r] ( log["code", @r{"status"}, str_len[@r{"stderr"}] > 0] )]
@os.run_async["definitely-not-a-command-xyz", {}, [@r] ( log[err_kind[@r]] )]
log["started"]
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "started");
    assert!(lines.contains(&"code 2 true"), "{out}");
    assert!(lines.contains(&"not_found"), "{out}");
}
