//! `std:http`: request, get, post, put, delete（design.md 5.11）
//!
//! `ureq`（rustls）を使う、ブロッキングの HTTP クライアント。`http` feature で有効になる。
//! 通信の失敗はエラー値、`404` などは普通のレスポンスとして返す

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use ureq::http;

use super::json::to_json_string;
use super::{error_value, expect_args, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:http",
        &[
            ("http.request", request),
            ("http.get", get),
            ("http.post", post),
            ("http.put", put),
            ("http.delete", delete),
        ],
    );
}

/// 本文の大きさの上限（バイト）
const MAX_BODY: u64 = 32 * 1024 * 1024;
const DEFAULT_TIMEOUT: f64 = 30.0;
const DEFAULT_REDIRECTS: u32 = 5;

fn request(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("http.request", args, 1)?;
    let Value::Map(options) = &args[0] else {
        return Err(type_error("http.request", 0, "マップ", &args[0]));
    };
    let options = options.borrow();
    let url = match options.get("url") {
        Some(Value::Str(url)) => url.to_string(),
        Some(other) => return Err(option_type("http.request", "url", "文字列", other)),
        None => return Err("http.request に `url` がありません".into()),
    };
    let method = match options.get("method") {
        Some(Value::Str(m)) => m.to_uppercase(),
        Some(other) => return Err(option_type("http.request", "method", "文字列", other)),
        None => "GET".to_string(),
    };
    let req = Req::from_options("http.request", url, method, &options, &["url", "method"])?;
    Ok(send(req))
}

fn get(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    with_method("http.get", "GET", args)
}

fn post(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    with_method("http.post", "POST", args)
}

fn put(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    with_method("http.put", "PUT", args)
}

fn delete(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    with_method("http.delete", "DELETE", args)
}

/// `get[url]` / `get[url, options]` の共通処理
fn with_method(name: &str, method: &str, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "{name} の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let Value::Str(url) = &args[0] else {
        return Err(type_error(name, 0, "文字列", &args[0]));
    };
    let empty = Map::default();
    let guard;
    let options: &Map = match args.get(1) {
        Some(Value::Map(options)) => {
            guard = options.borrow();
            &guard
        }
        Some(other) => return Err(type_error(name, 1, "マップ", other)),
        None => &empty,
    };
    let req = Req::from_options(name, url.to_string(), method.to_string(), options, &[])?;
    Ok(send(req))
}

/// 検査を終えたリクエスト
struct Req {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout: Duration,
    redirects: u32,
}

impl Req {
    /// `options` のキーを検査してリクエストにする。`skip` は呼び出し側がすでに読んだキー
    fn from_options(
        name: &str,
        mut url: String,
        method: String,
        options: &Map,
        skip: &[&str],
    ) -> Result<Req, String> {
        const KNOWN: [&str; 8] = [
            "url",
            "method",
            "headers",
            "query",
            "body",
            "json",
            "timeout",
            "redirects",
        ];
        for (key, _) in options.iter() {
            if !KNOWN.contains(&&**key) {
                return Err(format!(
                    "{name} に知らないオプション `{key}` があります（使えるのは {}）",
                    KNOWN[2..].join("、")
                ));
            }
            if (key.as_ref() == "url" || key.as_ref() == "method") && !skip.contains(&&**key) {
                return Err(format!("{name} では `{key}` を指定できません"));
            }
        }

        let mut headers = Vec::new();
        if let Some(value) = options.get("headers") {
            let Value::Map(map) = value else {
                return Err(option_type(name, "headers", "マップ", value));
            };
            for (key, value) in map.borrow().iter() {
                let Value::Str(value) = value else {
                    return Err(format!(
                        "{name} の `headers` の `{key}` は文字列です（{}が渡されました）",
                        value.type_name()
                    ));
                };
                headers.push((key.to_string(), value.to_string()));
            }
        }

        if let Some(value) = options.get("query") {
            let Value::Map(map) = value else {
                return Err(option_type(name, "query", "マップ", value));
            };
            let mut pairs = Vec::new();
            for (key, value) in map.borrow().iter() {
                let text = match value {
                    Value::Str(_) | Value::Num(_) | Value::Bool(_) => value.to_string(),
                    other => {
                        return Err(format!(
                            "{name} の `query` の `{key}` は文字列・数値・真偽値です（{}が渡されました）",
                            other.type_name()
                        ));
                    }
                };
                pairs.push(format!("{}={}", percent_encode(key), percent_encode(&text)));
            }
            if !pairs.is_empty() {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str(&pairs.join("&"));
            }
        }

        let body = match (options.get("body"), options.get("json")) {
            (Some(_), Some(_)) => {
                return Err(format!("{name} の `body` と `json` は同時に指定できません"));
            }
            (Some(Value::Str(body)), None) => Some(body.to_string()),
            (Some(other), None) => return Err(option_type(name, "body", "文字列", other)),
            (None, Some(value)) => {
                if !headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                {
                    headers.push(("Content-Type".into(), "application/json".into()));
                }
                Some(to_json_string(name, value, None)?)
            }
            (None, None) => None,
        };

        let timeout = match options.get("timeout") {
            Some(Value::Num(n)) if n.is_finite() && *n > 0.0 => {
                Duration::try_from_secs_f64(*n).unwrap_or(Duration::MAX)
            }
            Some(Value::Num(n)) => {
                return Err(format!("{name} の `timeout` は 0 より大きい数です（{n}）"));
            }
            Some(other) => return Err(option_type(name, "timeout", "数値", other)),
            None => Duration::from_secs_f64(DEFAULT_TIMEOUT),
        };
        let redirects = match options.get("redirects") {
            Some(Value::Num(n)) if n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX) => {
                *n as u32
            }
            Some(Value::Num(n)) => {
                return Err(format!("{name} の `redirects` は 0 以上の整数です（{n}）"));
            }
            Some(other) => return Err(option_type(name, "redirects", "数値", other)),
            None => DEFAULT_REDIRECTS,
        };

        Ok(Req {
            url,
            method,
            headers,
            body,
            timeout,
            redirects,
        })
    }
}

fn option_type(name: &str, key: &str, expected: &str, got: &Value) -> String {
    format!(
        "{name} の `{key}` は{expected}です（{}が渡されました）",
        got.type_name()
    )
}

/// 予約されていない文字以外を `%XX` にする（クエリのキーと値用）
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// リクエストを送り、レスポンスのマップか、通信の失敗のエラー値を返す
fn send(req: Req) -> Value {
    if req.url.parse::<http::Uri>().is_err() {
        return error_value("url", format!("URL が正しくありません: {}", req.url));
    }
    let mut builder = http::Request::builder()
        .method(req.method.as_str())
        .uri(req.url.as_str());
    for (key, value) in &req.headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(req.timeout))
        .max_redirects(req.redirects)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .user_agent(concat!("dolphin/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();

    let result = match req.body {
        Some(body) => match builder.body(body) {
            Ok(request) => agent.run(request),
            Err(e) => return request_error(&e),
        },
        None => match builder.body(()) {
            Ok(request) => agent.run(request),
            Err(e) => return request_error(&e),
        },
    };
    match result.and_then(read_response) {
        Ok(value) => value,
        Err(e) => failure(&e, &req.url),
    }
}

fn request_error(e: &http::Error) -> Value {
    error_value("request", format!("リクエストが正しくありません: {e}"))
}

/// レスポンスをマップにする。本文は UTF-8 の文字列として読む
fn read_response(mut response: http::Response<ureq::Body>) -> Result<Value, ureq::Error> {
    let status = response.status().as_u16();
    let mut headers = Map::default();
    for (name, value) in response.headers() {
        let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
        let merged = match headers.get(name.as_str()) {
            Some(Value::Str(old)) => format!("{old}, {value}"),
            _ => value,
        };
        headers.insert(name.as_str().into(), Value::Str(merged.into()));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()?;

    let mut map = Map::default();
    map.insert("status".into(), Value::Num(f64::from(status)));
    map.insert("ok".into(), Value::Bool((200..300).contains(&status)));
    map.insert("headers".into(), Value::Map(Rc::new(RefCell::new(headers))));
    map.insert("body".into(), Value::Str(body.into()));
    Ok(Value::Map(Rc::new(RefCell::new(map))))
}

/// `ureq` のエラーを、種類つきのエラー値にする
fn failure(e: &ureq::Error, url: &str) -> Value {
    use ureq::Error as E;
    let (kind, reason) = match e {
        E::Timeout(_) => ("timeout", "時間切れになりました".to_string()),
        E::BadUri(_) => ("url", "URL が正しくありません".to_string()),
        E::Http(e) => ("request", format!("リクエストが正しくありません: {e}")),
        E::BodyExceedsLimit(_) => ("too_large", "本文が大きすぎます（32 MB まで）".to_string()),
        E::Io(io_error) if io_error.kind() == io::ErrorKind::InvalidData => {
            ("parse", "本文が UTF-8 ではありません".to_string())
        }
        E::HostNotFound => ("network", "名前を引けません".to_string()),
        E::ConnectionFailed => ("network", "接続できません".to_string()),
        E::TooManyRedirects => ("network", "リダイレクトが多すぎます".to_string()),
        other => ("network", other.to_string()),
    };
    error_value(kind, format!("`{url}` への通信に失敗しました: {reason}"))
}
