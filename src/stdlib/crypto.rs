//! `std:crypto`: hash, hmac, random_bytes, uuid, equals（design.md 5.14）
//!
//! 暗号は自前で書かない。ハッシュ・HMAC・乱数は `ring`、md5 は `md-5` を使う。
//! `crypto` feature（既定で有効）

use md5::{Digest, Md5};
use ring::{digest, hmac, rand};

use super::encoding::hex_encode;
use super::{data_arg, expect_args, int_arg, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:crypto",
        &[
            ("crypto.hash", hash),
            ("crypto.hmac", hmac_fn),
            ("crypto.random_bytes", random_bytes),
            ("crypto.uuid", uuid),
            ("crypto.equals", equals),
        ],
    );
}

fn digest_algorithm(name: &str) -> Option<&'static digest::Algorithm> {
    Some(match name {
        "sha1" => &digest::SHA1_FOR_LEGACY_USE_ONLY,
        "sha256" => &digest::SHA256,
        "sha384" => &digest::SHA384,
        "sha512" => &digest::SHA512,
        _ => return None,
    })
}

fn unknown_algorithm(function: &str, name: &str, available: &str) -> String {
    format!("{function} の方式 {name:?} は使えません（使えるのは {available}）")
}

/// ダイジェスト（バイト列）
fn hash(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("crypto.hash", args, 2)?;
    let algorithm = str_arg("crypto.hash", args, 0)?;
    let data = data_arg("crypto.hash", args, 1)?;
    if algorithm == "md5" {
        return Ok(Value::bytes(Md5::digest(&*data).as_slice()));
    }
    let Some(algorithm_ref) = digest_algorithm(algorithm) else {
        return Err(unknown_algorithm(
            "crypto.hash",
            algorithm,
            "sha1、sha256、sha384、sha512、md5",
        ));
    };
    Ok(Value::bytes(digest::digest(algorithm_ref, &data).as_ref()))
}

/// HMAC（バイト列）
fn hmac_fn(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("crypto.hmac", args, 3)?;
    let algorithm = str_arg("crypto.hmac", args, 0)?;
    let key = data_arg("crypto.hmac", args, 1)?;
    let data = data_arg("crypto.hmac", args, 2)?;
    let algorithm = match algorithm {
        "sha1" => hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        "sha256" => hmac::HMAC_SHA256,
        "sha384" => hmac::HMAC_SHA384,
        "sha512" => hmac::HMAC_SHA512,
        other => {
            return Err(unknown_algorithm(
                "crypto.hmac",
                other,
                "sha1、sha256、sha384、sha512",
            ));
        }
    };
    let key = hmac::Key::new(algorithm, &key);
    Ok(Value::bytes(hmac::sign(&key, &data).as_ref()))
}

const RANDOM_MAX: i64 = 65_536;

/// OS の乱数で作ったバイト列
fn random_bytes(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("crypto.random_bytes", args, 1)?;
    let n = int_arg("crypto.random_bytes", args, 0)?;
    if !(0..=RANDOM_MAX).contains(&n) {
        return Err(format!(
            "crypto.random_bytes の大きさは 0〜{RANDOM_MAX} です（{n}）"
        ));
    }
    let mut bytes = vec![0u8; n as usize];
    fill_random(&mut bytes)?;
    Ok(Value::bytes(bytes))
}

fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    rand::SecureRandom::fill(&rand::SystemRandom::new(), bytes)
        .map_err(|_| "OS の乱数を取得できませんでした".to_string())
}

/// UUID v4
fn uuid(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("crypto.uuid", args, 0)?;
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes)?;
    // バージョン 4、バリアントは RFC 4122
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex = hex_encode(&bytes);
    Ok(Value::Str(
        format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )
        .into(),
    ))
}

/// 中身によらず同じ時間で比べる（HMAC の検証用）。長さが違えば、すぐ false
fn equals(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("crypto.equals", args, 2)?;
    let a = data_arg("crypto.equals", args, 0)?;
    let b = data_arg("crypto.equals", args, 1)?;
    Ok(Value::Bool(constant_time_equals(&a, &b)))
}

/// 違うバイトがあっても途中で止めず、最後まで調べる
fn constant_time_equals(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    // コンパイラが「差が出た時点で止める」形に最適化しないようにする
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_hash(algorithm: &str, data: &[u8]) -> String {
        if algorithm == "md5" {
            return hex_encode(Md5::digest(data).as_slice());
        }
        hex_encode(digest::digest(digest_algorithm(algorithm).unwrap(), data).as_ref())
    }

    #[test]
    fn known_digests() {
        assert_eq!(hex_hash("md5", b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex_hash("md5", b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex_hash("sha1", b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex_hash("sha256", b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_hash("sha512", b"abc").get(..32),
            Some("ddaf35a193617abacc417349ae204131")
        );
    }

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_equals(b"abc", b"abc"));
        assert!(constant_time_equals(b"", b""));
        assert!(!constant_time_equals(b"abc", b"abd"));
        assert!(!constant_time_equals(b"abc", b"ab"));
    }

    #[test]
    fn hmac_rfc4231_case_1() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, &[0x0b; 20]);
        let tag = hmac::sign(&key, b"Hi There");
        assert_eq!(
            hex_encode(tag.as_ref()),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }
}
