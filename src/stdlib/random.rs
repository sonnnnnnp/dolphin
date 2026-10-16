//! `std:random`: seed, choice, shuffle, sample, uniform, normal（design.md 5.31）
//!
//! 乱数列は `random[]` と同じもの（math.rs の xorshift64*）。暗号用途には使えない

use super::math::{below, next_f64, reseed};
use super::{array_arg, expect_args, int_arg, new_array, num_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:random",
        &[
            ("random.seed", seed),
            ("random.choice", choice),
            ("random.shuffle", shuffle),
            ("random.sample", sample),
            ("random.uniform", uniform),
            ("random.normal", normal),
        ],
    );
}

fn seed(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    match args.len() {
        0 => reseed(None),
        1 => reseed(Some(int_arg("random.seed", args, 0)?)),
        n => {
            return Err(format!(
                "random.seed の引数は 0 個か 1 個です（{n} 個渡されました）"
            ));
        }
    }
    Ok(Value::Nil)
}

fn choice(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random.choice", args, 1)?;
    let items = array_arg("random.choice", args, 0)?.borrow();
    if items.is_empty() {
        return Err("random.choice に空の配列は渡せません".into());
    }
    Ok(items[below(items.len() as u64) as usize].clone())
}

fn shuffle(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random.shuffle", args, 1)?;
    let mut items = array_arg("random.shuffle", args, 0)?.borrow().clone();
    // Fisher–Yates
    for i in (1..items.len()).rev() {
        items.swap(i, below(i as u64 + 1) as usize);
    }
    Ok(new_array(items))
}

fn sample(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random.sample", args, 2)?;
    let mut items = array_arg("random.sample", args, 0)?.borrow().clone();
    let k = int_arg("random.sample", args, 1)?;
    let len = items.len();
    let k = usize::try_from(k)
        .ok()
        .filter(|k| *k <= len)
        .ok_or_else(|| format!("random.sample の個数は 0〜{len} の整数です（{k}）"))?;
    // 先頭から k 個ぶんだけ Fisher–Yates を進める
    for i in 0..k {
        items.swap(i, i + below((len - i) as u64) as usize);
    }
    items.truncate(k);
    Ok(new_array(items))
}

fn finite_arg(name: &str, args: &[Value], i: usize) -> Result<f64, String> {
    let n = num_arg(name, args, i)?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!(
            "{name} の {} 番目の引数は有限の数です（{n}）",
            i + 1
        ))
    }
}

fn uniform(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random.uniform", args, 2)?;
    let a = finite_arg("random.uniform", args, 0)?;
    let b = finite_arg("random.uniform", args, 1)?;
    if a > b {
        return Err(format!("random.uniform の範囲が逆です（{a} > {b}）"));
    }
    Ok(Value::Num((a + (b - a) * next_f64()).min(b)))
}

fn normal(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("random.normal", args, 2)?;
    let mean = finite_arg("random.normal", args, 0)?;
    let stddev = finite_arg("random.normal", args, 1)?;
    if stddev < 0.0 {
        return Err(format!("random.normal の標準偏差は 0 以上です（{stddev}）"));
    }
    // Box–Muller。u1 は 0 を避ける（ln(0) を出さない）
    let u1 = 1.0 - next_f64();
    let u2 = next_f64();
    let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    Ok(Value::Num(mean + stddev * z))
}
