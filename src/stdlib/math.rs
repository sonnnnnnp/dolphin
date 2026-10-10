//! 数学: abs, floor, ceil, round, sqrt, pow, min, max, random, random_int

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{expect_args, int_arg, num_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("abs", |_, args| unary("abs", args, f64::abs));
    interp.register("floor", |_, args| unary("floor", args, f64::floor));
    interp.register("ceil", |_, args| unary("ceil", args, f64::ceil));
    // 0.5 は 0 から遠いほうへ丸める（2.5 → 3、-2.5 → -3）
    interp.register("round", |_, args| unary("round", args, f64::round));
    interp.register("sqrt", sqrt);
    interp.register("pow", pow);
    interp.register("min", |_, args| fold("min", args, f64::min));
    interp.register("max", |_, args| fold("max", args, f64::max));
    interp.register("random", random);
    interp.register("random_int", random_int);
}

/// 引数 1 つの数値関数の共通処理
fn unary(name: &str, args: &[Value], f: fn(f64) -> f64) -> Result<Value, String> {
    expect_args(name, args, 1)?;
    Ok(Value::Num(f(num_arg(name, args, 0)?)))
}

/// min / max。引数は 1 個以上
fn fold(name: &str, args: &[Value], f: fn(f64, f64) -> f64) -> Result<Value, String> {
    if args.is_empty() {
        return Err(format!("{name} には 1 個以上の数値を渡してください"));
    }
    let mut acc = num_arg(name, args, 0)?;
    for i in 1..args.len() {
        acc = f(acc, num_arg(name, args, i)?);
    }
    Ok(Value::Num(acc))
}

fn sqrt(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("sqrt", args, 1)?;
    let n = num_arg("sqrt", args, 0)?;
    if n < 0.0 {
        return Err(format!("負の数 {n} の平方根は求められません"));
    }
    Ok(Value::Num(n.sqrt()))
}

fn pow(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("pow", args, 2)?;
    let base = num_arg("pow", args, 0)?;
    let exp = num_arg("pow", args, 1)?;
    Ok(Value::Num(base.powf(exp)))
}

// ---- 乱数 ----
//
// コア言語は外部クレートを使わない方針なので、xorshift64* を自前で持つ。
// 暗号用途には使えないが、ゲームやサンプルには十分

thread_local! {
    static RNG_STATE: Cell<u64> = Cell::new(seed());
}

fn seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // 状態が 0 だと 0 しか出なくなるので、最下位ビットを立てる
    nanos | 1
}

fn next_u64() -> u64 {
    RNG_STATE.with(|state| {
        let mut x = state.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// 0 以上 1 未満
fn next_f64() -> f64 {
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// 0 以上 1 未満の数
fn random(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random", args, 0)?;
    Ok(Value::Num(next_f64()))
}

/// `min` 以上 `max` 以下の整数
fn random_int(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random_int", args, 2)?;
    let min = int_arg("random_int", args, 0)?;
    let max = int_arg("random_int", args, 1)?;
    if min > max {
        return Err(format!("random_int の範囲が逆です（{min} > {max}）"));
    }
    let span = (max - min + 1) as f64;
    Ok(Value::Num((min + (next_f64() * span) as i64) as f64))
}
