//! 文字列と変換
//!
//! 位置や長さはすべて文字単位（バイト単位ではない）。`"イルカ"` の長さは 3

use super::{array_arg, expect_args, int_arg, new_array, str_arg};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("str_len", str_len);
    interp.register("str_concat", str_concat);
    interp.register("str_upper", str_upper);
    interp.register("str_lower", str_lower);
    interp.register("str_trim", str_trim);
    interp.register("str_contains", str_contains);
    interp.register("str_find", str_find);
    interp.register("str_replace", str_replace);
    interp.register("str_split", str_split);
    interp.register("str_join", str_join);
    interp.register("str_sub", str_sub);
    interp.register("to_str", to_str);
    interp.register("to_num", to_num);
}

fn string(s: impl Into<std::rc::Rc<str>>) -> Value {
    Value::Str(s.into())
}

fn str_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_len", args, 1)?;
    let s = str_arg("str_len", args, 0)?;
    Ok(Value::Num(s.chars().count() as f64))
}

/// 値を文字列にしてつなげる。数値なども渡せる。引数の数は自由
fn str_concat(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let s: String = args.iter().map(Value::to_string).collect();
    Ok(string(s))
}

fn str_upper(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_upper", args, 1)?;
    Ok(string(str_arg("str_upper", args, 0)?.to_uppercase()))
}

fn str_lower(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_lower", args, 1)?;
    Ok(string(str_arg("str_lower", args, 0)?.to_lowercase()))
}

/// 前後の空白を取り除く
fn str_trim(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_trim", args, 1)?;
    Ok(string(str_arg("str_trim", args, 0)?.trim()))
}

fn str_contains(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_contains", args, 2)?;
    let s = str_arg("str_contains", args, 0)?;
    let sub = str_arg("str_contains", args, 1)?;
    Ok(Value::Bool(s.contains(sub)))
}

/// 最初に見つかった位置（文字単位）。なければ -1
fn str_find(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_find", args, 2)?;
    let s = str_arg("str_find", args, 0)?;
    let sub = str_arg("str_find", args, 1)?;
    let pos = match s.find(sub) {
        Some(byte_pos) => s[..byte_pos].chars().count() as f64,
        None => -1.0,
    };
    Ok(Value::Num(pos))
}

/// すべて置き換える
fn str_replace(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_replace", args, 3)?;
    let s = str_arg("str_replace", args, 0)?;
    let from = str_arg("str_replace", args, 1)?;
    let to = str_arg("str_replace", args, 2)?;
    if from.is_empty() {
        return Err("str_replace の 2 番目の引数に空文字列は使えません".into());
    }
    Ok(string(s.replace(from, to)))
}

/// 区切り文字で分けた配列。区切りが空文字列なら 1 文字ずつに分ける
fn str_split(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_split", args, 2)?;
    let s = str_arg("str_split", args, 0)?;
    let sep = str_arg("str_split", args, 1)?;
    let parts = if sep.is_empty() {
        s.chars().map(|c| string(c.to_string())).collect()
    } else {
        s.split(sep).map(string).collect()
    };
    Ok(new_array(parts))
}

/// 配列の要素を文字列にして、区切り文字でつなげる
fn str_join(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_join", args, 2)?;
    let items = array_arg("str_join", args, 0)?;
    let sep = str_arg("str_join", args, 1)?;
    let parts: Vec<String> = items.borrow().iter().map(Value::to_string).collect();
    Ok(string(parts.join(sep)))
}

/// `start` 文字目から `end` 文字目の手前まで（`end` は含まない）
fn str_sub(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_sub", args, 3)?;
    let s = str_arg("str_sub", args, 0)?;
    let start = int_arg("str_sub", args, 1)?;
    let end = int_arg("str_sub", args, 2)?;
    let len = s.chars().count() as i64;
    if !(0 <= start && start <= end && end <= len) {
        return Err(format!(
            "str_sub の範囲 {start}〜{end} は使えません（0 ≦ 開始 ≦ 終了 ≦ {len}）"
        ));
    }
    let sub: String = s
        .chars()
        .skip(start as usize)
        .take((end - start) as usize)
        .collect();
    Ok(string(sub))
}

/// `log` と同じ表示の文字列にする
fn to_str(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("to_str", args, 1)?;
    Ok(string(args[0].to_string()))
}

/// 文字列を数値にする（前後の空白は無視）。数値はそのまま返す
fn to_num(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("to_num", args, 1)?;
    match &args[0] {
        Value::Num(n) => Ok(Value::Num(*n)),
        Value::Str(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Value::Num)
            .ok_or_else(|| format!("数値に変換できません（{s:?}）")),
        other => Err(format!(
            "to_num の 1 番目の引数は文字列か数値です（{}が渡されました）",
            other.type_name()
        )),
    }
}
