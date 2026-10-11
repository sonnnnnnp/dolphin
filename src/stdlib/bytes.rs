//! バイト列: str_to_bytes, bytes_from_arr, bytes_new, bytes_concat, bytes_len, bytes_slice,
//! bytes_to_arr, bytes_find, bytes_to_str（design.md 5.14）
//!
//! バイト列は変更できない（`Value::Bytes`）。読み取りは `@b{i}`

use super::{
    array_arg, bytes_arg, data_arg, error_value, expect_args, int_arg, new_array, str_arg,
};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("str_to_bytes", str_to_bytes);
    interp.register("bytes_from_arr", bytes_from_arr);
    interp.register("bytes_new", bytes_new);
    interp.register("bytes_concat", bytes_concat);
    interp.register("bytes_len", bytes_len);
    interp.register("bytes_slice", bytes_slice);
    interp.register("bytes_to_arr", bytes_to_arr);
    interp.register("bytes_find", bytes_find);
    interp.register("bytes_to_str", bytes_to_str);
}

/// 作れるバイト列の大きさの上限
const BYTES_MAX: i64 = 100_000_000;

/// 文字列を UTF-8 のバイト列にする
fn str_to_bytes(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_to_bytes", args, 1)?;
    Ok(Value::bytes(str_arg("str_to_bytes", args, 0)?.as_bytes()))
}

/// 0〜255 の整数の配列から作る
fn bytes_from_arr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_from_arr", args, 1)?;
    let items = array_arg("bytes_from_arr", args, 0)?.borrow();
    let mut bytes = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Num(n) if n.fract() == 0.0 && (0.0..=255.0).contains(n) => bytes.push(*n as u8),
            Value::Num(n) => {
                return Err(format!(
                    "bytes_from_arr の {i} 番目は 0〜255 の整数です（{n}）"
                ));
            }
            other => {
                return Err(format!(
                    "bytes_from_arr の {i} 番目は数値です（{}が渡されました）",
                    other.type_name()
                ));
            }
        }
    }
    Ok(Value::bytes(bytes))
}

/// 0 で埋めた `n` バイト
fn bytes_new(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_new", args, 1)?;
    let n = int_arg("bytes_new", args, 0)?;
    if !(0..=BYTES_MAX).contains(&n) {
        return Err(format!("bytes_new の大きさは 0〜{BYTES_MAX} です（{n}）"));
    }
    Ok(Value::bytes(vec![0u8; n as usize]))
}

/// つなげる。どちらも、バイト列か文字列（UTF-8）
fn bytes_concat(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_concat", args, 2)?;
    let mut bytes = data_arg("bytes_concat", args, 0)?.into_owned();
    bytes.extend_from_slice(&data_arg("bytes_concat", args, 1)?);
    Ok(Value::bytes(bytes))
}

fn bytes_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_len", args, 1)?;
    Ok(Value::Num(bytes_arg("bytes_len", args, 0)?.len() as f64))
}

/// `start` から `end` の手前まで
fn bytes_slice(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_slice", args, 3)?;
    let bytes = bytes_arg("bytes_slice", args, 0)?;
    let start = int_arg("bytes_slice", args, 1)?;
    let end = int_arg("bytes_slice", args, 2)?;
    let len = bytes.len() as i64;
    if !(0 <= start && start <= end && end <= len) {
        return Err(format!(
            "bytes_slice の範囲 {start}〜{end} は使えません（0 ≦ 開始 ≦ 終了 ≦ {len}）"
        ));
    }
    Ok(Value::bytes(&bytes[start as usize..end as usize]))
}

/// 0〜255 の数値の配列にする
fn bytes_to_arr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_to_arr", args, 1)?;
    let bytes = bytes_arg("bytes_to_arr", args, 0)?;
    Ok(new_array(
        bytes.iter().map(|&b| Value::Num(f64::from(b))).collect(),
    ))
}

/// 最初に見つかった位置（バイト単位）。なければ -1
fn bytes_find(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_find", args, 2)?;
    let bytes = bytes_arg("bytes_find", args, 0)?;
    let sub = data_arg("bytes_find", args, 1)?;
    let pos = if sub.is_empty() {
        Some(0)
    } else {
        bytes.windows(sub.len()).position(|w| w == &*sub)
    };
    Ok(Value::Num(pos.map_or(-1.0, |p| p as f64)))
}

/// UTF-8 の文字列にする。UTF-8 でなければ `parse` のエラー値
fn bytes_to_str(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bytes_to_str", args, 1)?;
    let bytes = bytes_arg("bytes_to_str", args, 0)?;
    Ok(match std::str::from_utf8(bytes) {
        Ok(s) => Value::Str(s.into()),
        Err(e) => error_value(
            "parse",
            format!("UTF-8 として読めません（{} バイト目）", e.valid_up_to() + 1),
        ),
    })
}
