//! 数学: abs, floor, ceil, round, sqrt, pow, min, max, random, random_int,
//! trunc, sign, clamp, num_fixed。`std:math`（定数・三角関数・対数。design.md 5.12）

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
    interp.register("trunc", |_, args| unary("trunc", args, f64::trunc));
    interp.register("sign", |_, args| unary("sign", args, sign));
    interp.register("clamp", clamp);
    interp.register("num_fixed", num_fixed);
    register_std_math(interp);
}

/// `import["std:math"]`。変数の `pi` `e` `inf` `nan` と、三角関数・対数など
fn register_std_math(interp: &mut Interpreter<'_>) {
    use std::f64::consts;
    interp.register_module_constants(
        "std:math",
        &[
            ("pi", consts::PI),
            ("e", consts::E),
            ("inf", f64::INFINITY),
            ("nan", f64::NAN),
        ],
    );
    interp.register_module(
        "std:math",
        &[
            ("math.sin", |_, args| unary("math.sin", args, f64::sin)),
            ("math.cos", |_, args| unary("math.cos", args, f64::cos)),
            ("math.tan", |_, args| unary("math.tan", args, f64::tan)),
            ("math.asin", |_, args| unary("math.asin", args, f64::asin)),
            ("math.acos", |_, args| unary("math.acos", args, f64::acos)),
            ("math.atan", |_, args| unary("math.atan", args, f64::atan)),
            ("math.atan2", |_, args| {
                binary("math.atan2", args, f64::atan2)
            }),
            ("math.exp", |_, args| unary("math.exp", args, f64::exp)),
            ("math.ln", |_, args| unary("math.ln", args, f64::ln)),
            ("math.log10", |_, args| {
                unary("math.log10", args, f64::log10)
            }),
            ("math.log2", |_, args| unary("math.log2", args, f64::log2)),
            ("math.hypot", |_, args| {
                binary("math.hypot", args, f64::hypot)
            }),
            ("math.is_nan", |_, args| {
                expect_args("math.is_nan", args, 1)?;
                Ok(Value::Bool(num_arg("math.is_nan", args, 0)?.is_nan()))
            }),
            ("math.is_finite", |_, args| {
                expect_args("math.is_finite", args, 1)?;
                Ok(Value::Bool(num_arg("math.is_finite", args, 0)?.is_finite()))
            }),
        ],
    );
}

/// 引数 2 つの数値関数の共通処理
fn binary(name: &str, args: &[Value], f: fn(f64, f64) -> f64) -> Result<Value, String> {
    expect_args(name, args, 2)?;
    Ok(Value::Num(f(
        num_arg(name, args, 0)?,
        num_arg(name, args, 1)?,
    )))
}

/// -1 / 0 / 1。NaN は NaN
fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else {
        x.signum()
    }
}

/// `x` を `lo` 以上 `hi` 以下に収める
fn clamp(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("clamp", args, 3)?;
    let (x, lo, hi) = (
        num_arg("clamp", args, 0)?,
        num_arg("clamp", args, 1)?,
        num_arg("clamp", args, 2)?,
    );
    if lo.is_nan() || hi.is_nan() || lo > hi {
        return Err(format!(
            "clamp の範囲 {lo}〜{hi} は使えません（下限 ≦ 上限）"
        ));
    }
    Ok(Value::Num(x.clamp(lo, hi)))
}

/// 小数点以下 `digits` 桁（0〜20）の文字列にする。ちょうど半分は 0 から遠いほうへ丸める
fn num_fixed(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("num_fixed", args, 2)?;
    let n = num_arg("num_fixed", args, 0)?;
    let digits = int_arg("num_fixed", args, 1)?;
    if !n.is_finite() {
        return Err(format!("num_fixed の 1 番目の引数は有限の数値です（{n}）"));
    }
    if !(0..=20).contains(&digits) {
        return Err(format!("num_fixed の桁数は 0〜20 です（{digits}）"));
    }
    let d = digits as usize;
    // Rust の書式は、ちょうど半分を偶数側に丸める。ちょうど半分かを、さらに 25 桁見て調べ、
    // そうなら 1 ulp だけ 0 から遠ざけて、遠いほうへ丸まるようにする
    let probe = format!("{:.*}", d + 25, n.abs());
    let tail = &probe[probe.len() - 25..];
    let is_tie = tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0');
    let n = if is_tie {
        f64::from_bits(n.to_bits() + 1)
    } else {
        n
    };
    let mut out = format!("{n:.d$}");
    // 丸めて 0 になった負の数は、`-0.00` ではなく `0.00`
    if out.starts_with('-') && out[1..].bytes().all(|b| b == b'0' || b == b'.') {
        out.remove(0);
    }
    Ok(Value::Str(out.into()))
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
