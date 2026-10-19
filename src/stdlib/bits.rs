//! `std:bits`: and, or, xor, not, shl, shr, popcount（design.md 5.19）
//!
//! 数値（64 ビット浮動小数点数）のうち ±2^53 以内の整数を、64 ビットの 2 の補数として
//! 計算する。結果が ±2^53 を超えるときは実行時エラー。引数が整数（`int`。design.md 5.40）
//! なら、制限なしで 64 ビットのまま計算して整数を返す

use super::{expect_args, int_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

/// 浮動小数点数で正確に表せる整数の範囲
const LIMIT: i64 = 1 << 53;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:bits",
        &[
            ("bits.and", |_, args| binary("bits.and", args, |a, b| a & b)),
            ("bits.or", |_, args| binary("bits.or", args, |a, b| a | b)),
            ("bits.xor", |_, args| binary("bits.xor", args, |a, b| a ^ b)),
            ("bits.not", not),
            ("bits.shl", shl),
            ("bits.shr", shr),
            ("bits.popcount", popcount),
        ],
    );
}

/// ±2^53 以内の整数の引数
fn bits_arg(name: &str, args: &[Value], i: usize) -> Result<i64, String> {
    let n = int_arg(name, args, i)?;
    if (-LIMIT..=LIMIT).contains(&n) {
        Ok(n)
    } else {
        Err(format!(
            "{name} の {} 番目の引数は ±2^53 以内の整数です（{n} が渡されました）",
            i + 1
        ))
    }
}

fn result(name: &str, n: i128) -> Result<Value, String> {
    if (i128::from(-LIMIT)..=i128::from(LIMIT)).contains(&n) {
        Ok(Value::Num(n as f64))
    } else {
        Err(format!("{name} の結果 {n} は ±2^53 を超えます"))
    }
}

/// 整数と整数以外が混ざっていたらエラー（整数を渡したときは、すべて整数）
fn same_kind(name: &str, args: &[Value]) -> Result<bool, String> {
    let ints = args.iter().filter(|v| matches!(v, Value::Int(_))).count();
    if ints == 0 || ints == args.len() {
        return Ok(ints > 0);
    }
    let other = args
        .iter()
        .find(|v| !matches!(v, Value::Int(_)))
        .map_or("", Value::type_name);
    Err(format!("{name} の引数に整数と{other}が混ざっています"))
}

fn int_value(args: &[Value], i: usize) -> i64 {
    match args[i] {
        Value::Int(n) => n,
        _ => unreachable!("same_kind で整数だと確かめた"),
    }
}

fn binary(name: &str, args: &[Value], f: fn(i64, i64) -> i64) -> Result<Value, String> {
    expect_args(name, args, 2)?;
    if same_kind(name, args)? {
        return Ok(Value::Int(f(int_value(args, 0), int_value(args, 1))));
    }
    let (a, b) = (bits_arg(name, args, 0)?, bits_arg(name, args, 1)?);
    result(name, i128::from(f(a, b)))
}

fn not(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bits.not", args, 1)?;
    if let Value::Int(n) = args[0] {
        return Ok(Value::Int(!n));
    }
    result("bits.not", i128::from(!bits_arg("bits.not", args, 0)?))
}

/// シフトの量（0〜63）
fn shift_arg(name: &str, args: &[Value]) -> Result<u32, String> {
    let n = int_arg(name, args, 1)?;
    u32::try_from(n)
        .ok()
        .filter(|n| *n < 64)
        .ok_or_else(|| format!("{name} のシフトの量は 0〜63 です（{n} が渡されました）"))
}

/// 整数のシフトの量（0〜63）
fn int_shift(name: &str, args: &[Value]) -> Result<u32, String> {
    same_kind(name, args)?;
    let n = int_value(args, 1);
    u32::try_from(n)
        .ok()
        .filter(|n| *n < 64)
        .ok_or_else(|| format!("{name} のシフトの量は 0〜63 です（{n} が渡されました）"))
}

fn shl(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bits.shl", args, 2)?;
    if same_kind("bits.shl", args)? {
        let n = int_shift("bits.shl", args)?;
        return Ok(Value::Int(int_value(args, 0) << n));
    }
    let a = bits_arg("bits.shl", args, 0)?;
    let n = shift_arg("bits.shl", args)?;
    result("bits.shl", i128::from(a) << n)
}

fn shr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bits.shr", args, 2)?;
    if same_kind("bits.shr", args)? {
        let n = int_shift("bits.shr", args)?;
        return Ok(Value::Int(int_value(args, 0) >> n));
    }
    let a = bits_arg("bits.shr", args, 0)?;
    let n = shift_arg("bits.shr", args)?;
    result("bits.shr", i128::from(a >> n))
}

fn popcount(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("bits.popcount", args, 1)?;
    if let Value::Int(n) = args[0] {
        return Ok(Value::Int(i64::from(n.count_ones())));
    }
    let a = bits_arg("bits.popcount", args, 0)?;
    if a < 0 {
        return Err(format!(
            "bits.popcount の引数は 0 以上の整数です（{a} が渡されました）"
        ));
    }
    Ok(Value::Num(f64::from(a.count_ones())))
}
