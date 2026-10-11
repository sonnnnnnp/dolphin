//! `std:encoding`: base64_encode, base64_decode, base64url_encode, base64url_decode,
//! hex_encode, hex_decode, url_encode, url_decode（design.md 5.14）
//!
//! 自前の実装。引数は、バイト列か文字列（UTF-8 として扱う）

use super::{data_arg, error_value, expect_args, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:encoding",
        &[
            ("encoding.base64_encode", |_, args| {
                encode("encoding.base64_encode", args, |b| base64_encode(b, false))
            }),
            ("encoding.base64_decode", |_, args| {
                decode("encoding.base64_decode", args, |s| base64_decode(s, false))
            }),
            ("encoding.base64url_encode", |_, args| {
                encode("encoding.base64url_encode", args, |b| {
                    base64_encode(b, true)
                })
            }),
            ("encoding.base64url_decode", |_, args| {
                decode("encoding.base64url_decode", args, |s| {
                    base64_decode(s, true)
                })
            }),
            ("encoding.hex_encode", |_, args| {
                encode("encoding.hex_encode", args, hex_encode)
            }),
            ("encoding.hex_decode", |_, args| {
                decode("encoding.hex_decode", args, hex_decode)
            }),
            ("encoding.url_encode", url_encode_fn),
            ("encoding.url_decode", url_decode_fn),
        ],
    );
}

/// データを文字列にする関数の共通処理
fn encode(name: &str, args: &[Value], f: fn(&[u8]) -> String) -> Result<Value, String> {
    expect_args(name, args, 1)?;
    Ok(Value::Str(f(&data_arg(name, args, 0)?).into()))
}

/// 文字列をバイト列にする関数の共通処理。失敗は `parse` のエラー値
fn decode(
    name: &str,
    args: &[Value],
    f: fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Value, String> {
    expect_args(name, args, 1)?;
    let s = str_arg(name, args, 0)?;
    Ok(match f(s) {
        Ok(bytes) => Value::bytes(bytes),
        Err(message) => error_value("parse", message),
    })
}

// ---- base64（RFC 4648） ----

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `url` なら `-` `_` を使い、`=` の詰め物をしない
fn base64_encode(bytes: &[u8], url: bool) -> String {
    let table = if url { URL_SAFE } else { STANDARD };
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..=chunk.len() {
            out.push(char::from(table[(n >> (18 - 6 * i) & 63) as usize]));
        }
        if !url {
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
    }
    out
}

/// 空白と改行は無視する。標準は `=` の詰め物を受け付け（あってもなくてもよい）、
/// `url` は `-` `_` を使う。詰め物の位置や文字が正しくなければエラー
fn base64_decode(s: &str, url: bool) -> Result<Vec<u8>, String> {
    let table = if url { URL_SAFE } else { STANDARD };
    let mut values = Vec::with_capacity(s.len());
    let mut padding = 0;
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == '=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err(format!(
                "base64 の `=` の後に文字があります（{} 文字目）",
                i + 1
            ));
        }
        let value = u8::try_from(c)
            .ok()
            .and_then(|b| table.iter().position(|&t| t == b))
            .ok_or_else(|| format!("base64 に使えない文字 {c:?} があります（{} 文字目）", i + 1))?;
        values.push(value as u32);
    }
    // 余りが 1 文字になることはない（6 ビットでは 1 バイトに足りない）
    if values.len() % 4 == 1 || padding > 2 {
        return Err("base64 の長さが正しくありません".into());
    }
    if padding > 0 && (values.len() + padding) % 4 != 0 {
        return Err("base64 の `=` の数が正しくありません".into());
    }
    let mut out = Vec::with_capacity(values.len() / 4 * 3 + 2);
    for chunk in values.chunks(4) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, v)| acc | v << (18 - 6 * i));
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

// ---- 16 進数 ----

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("16 進数の桁数が奇数です".into());
    }
    let digit = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    s.as_bytes()
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| match (digit(pair[0]), digit(pair[1])) {
            (Some(hi), Some(lo)) => Ok(hi << 4 | lo),
            _ => Err(format!(
                "16 進数に使えない文字があります（{} 文字目付近）",
                i * 2 + 1
            )),
        })
        .collect()
}

// ---- URL のパーセントエンコード ----

/// 英数字と `-` `.` `_` `~` 以外を `%XX` にする
pub(super) fn url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_encode_fn(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    encode("encoding.url_encode", args, url_encode)
}

/// `%XX` を戻す。`+` は空白にしない。UTF-8 にならなければ `parse` のエラー値
fn url_decode_fn(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("encoding.url_decode", args, 1)?;
    let s = str_arg("encoding.url_decode", args, 0)?;
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            // `from_str_radix` は `+` も受け付けるので、16 進数の桁だけか確かめる
            Some(b) if hex.is_some_and(|h| h.bytes().all(|c| c.is_ascii_hexdigit())) => {
                out.push(b);
                i += 3;
            }
            _ => {
                return Ok(error_value(
                    "parse",
                    format!("`%` の後に 16 進数 2 桁が必要です（{} バイト目）", i + 1),
                ));
            }
        }
    }
    Ok(match String::from_utf8(out) {
        Ok(text) => Value::Str(text.into()),
        Err(_) => error_value("parse", "デコードした結果が UTF-8 ではありません"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes(), false), encoded);
            assert_eq!(base64_decode(encoded, false).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn base64_variants() {
        let bytes = [0xfb, 0xff, 0xfe];
        assert_eq!(base64_encode(&bytes, false), "+//+");
        assert_eq!(base64_encode(&bytes, true), "-__-");
        assert_eq!(base64_decode("-__-", true).unwrap(), bytes);
        // URL 向けは詰め物なし。復号は、詰め物があってもなくてもよい
        assert_eq!(base64_encode(b"f", true), "Zg");
        assert_eq!(base64_decode("Zg", true).unwrap(), b"f");
        assert_eq!(base64_decode("Zg", false).unwrap(), b"f");
        assert_eq!(base64_decode("Zg==", true).unwrap(), b"f");
        // 空白と改行は無視する
        assert_eq!(base64_decode("Zm9v\nYmFy \r\n", false).unwrap(), b"foobar");
    }

    #[test]
    fn base64_errors() {
        assert!(base64_decode("Zm9v!", false).is_err());
        assert!(base64_decode("Z", false).is_err());
        assert!(base64_decode("Zg=x", false).is_err());
        assert!(base64_decode("Zg===", false).is_err());
        assert!(base64_decode("Z===", false).is_err());
        // 標準とURL向けは、文字が違う
        assert!(base64_decode("-__-", false).is_err());
        assert!(base64_decode("+//+", true).is_err());
        assert!(base64_decode("日本", false).is_err());
    }

    #[test]
    fn hex() {
        assert_eq!(hex_encode(&[0, 15, 16, 255]), "000f10ff");
        assert_eq!(hex_decode("000F10ff").unwrap(), [0, 15, 16, 255]);
        assert_eq!(hex_decode("").unwrap(), Vec::<u8>::new());
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("+1").is_err());
    }

    #[test]
    fn url() {
        assert_eq!(
            url_encode("a b/日本~-._".as_bytes()),
            "a%20b%2F%E6%97%A5%E6%9C%AC~-._"
        );
    }
}
