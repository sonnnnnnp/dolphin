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
    interp.register("str_starts_with", str_starts_with);
    interp.register("str_ends_with", str_ends_with);
    interp.register("str_repeat", str_repeat);
    interp.register("str_pad_left", |_, args| pad("str_pad_left", args, true));
    interp.register("str_pad_right", |_, args| pad("str_pad_right", args, false));
    interp.register("str_trim_start", str_trim_start);
    interp.register("str_trim_end", str_trim_end);
    interp.register("str_lines", str_lines);
    interp.register("str_code_at", str_code_at);
    interp.register("str_from_code", str_from_code);
    interp.register("str_rfind", str_rfind);
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

/// 文字列を数値にする（前後の空白は無視）。数値はそのまま返す。
/// 数値として読めない文字列は、`parse` のエラー値を返す
fn to_num(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("to_num", args, 1)?;
    match &args[0] {
        Value::Num(n) => Ok(Value::Num(*n)),
        Value::Str(s) => Ok(s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Value::Num)
            .unwrap_or_else(|| {
                super::error_value("parse", format!("数値に変換できません（{s:?}）"))
            })),
        other => Err(format!(
            "to_num の 1 番目の引数は文字列か数値です（{}が渡されました）",
            other.type_name()
        )),
    }
}

fn str_starts_with(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_starts_with", args, 2)?;
    let s = str_arg("str_starts_with", args, 0)?;
    Ok(Value::Bool(s.starts_with(str_arg(
        "str_starts_with",
        args,
        1,
    )?)))
}

fn str_ends_with(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_ends_with", args, 2)?;
    let s = str_arg("str_ends_with", args, 0)?;
    Ok(Value::Bool(s.ends_with(str_arg("str_ends_with", args, 1)?)))
}

/// 作れる文字列の長さの上限（文字数）
const STR_MAX: usize = 100_000_000;

/// `s` を `n` 回つなげる
fn str_repeat(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_repeat", args, 2)?;
    let s = str_arg("str_repeat", args, 0)?;
    let n = int_arg("str_repeat", args, 1)?;
    if n < 0 {
        return Err(format!("str_repeat の回数は 0 以上です（{n}）"));
    }
    let total = s.chars().count().checked_mul(n as usize);
    if total.is_none_or(|t| t > STR_MAX) {
        return Err(format!(
            "str_repeat の結果が大きすぎます（{STR_MAX} 文字まで）"
        ));
    }
    Ok(string(s.repeat(n as usize)))
}

/// 文字数が `width` になるまで `fill`（1 文字。省略は空白）を足す。すでに `width` 以上ならそのまま
fn pad(name: &str, args: &[Value], left: bool) -> Result<Value, String> {
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "{name} の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let s = str_arg(name, args, 0)?;
    let width = int_arg(name, args, 1)?;
    let fill = match args.get(2) {
        Some(_) => {
            let fill = str_arg(name, args, 2)?;
            let mut chars = fill.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => c,
                _ => return Err(format!("{name} の 3 番目の引数は 1 文字です（{fill:?}）")),
            }
        }
        None => ' ',
    };
    if !(0..=STR_MAX as i64).contains(&width) {
        return Err(format!("{name} の幅は 0〜{STR_MAX} です（{width}）"));
    }
    let missing = (width as usize).saturating_sub(s.chars().count());
    let padding: String = std::iter::repeat_n(fill, missing).collect();
    Ok(string(if left {
        format!("{padding}{s}")
    } else {
        format!("{s}{padding}")
    }))
}

fn str_trim_start(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_trim_start", args, 1)?;
    Ok(string(str_arg("str_trim_start", args, 0)?.trim_start()))
}

fn str_trim_end(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_trim_end", args, 1)?;
    Ok(string(str_arg("str_trim_end", args, 0)?.trim_end()))
}

/// 改行（`\n` と `\r\n`）で分けた配列。末尾の改行で空の要素は作らない
fn str_lines(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_lines", args, 1)?;
    let s = str_arg("str_lines", args, 0)?;
    Ok(new_array(s.lines().map(string).collect()))
}

/// `i` 番目の文字のコードポイント
fn str_code_at(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_code_at", args, 2)?;
    let s = str_arg("str_code_at", args, 0)?;
    let i = int_arg("str_code_at", args, 1)?;
    let len = s.chars().count();
    match usize::try_from(i).ok().and_then(|i| s.chars().nth(i)) {
        Some(c) => Ok(Value::Num(f64::from(u32::from(c)))),
        None => Err(format!(
            "str_code_at の位置 {i} は範囲外です（文字は {len} 個）"
        )),
    }
}

/// コードポイントから 1 文字の文字列
fn str_from_code(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_from_code", args, 1)?;
    let n = int_arg("str_from_code", args, 0)?;
    match u32::try_from(n).ok().and_then(char::from_u32) {
        Some(c) => Ok(string(c.to_string())),
        None => Err(format!("str_from_code のコードポイント {n} は使えません")),
    }
}

/// 最後に見つかった位置（文字単位）。なければ -1
fn str_rfind(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("str_rfind", args, 2)?;
    let s = str_arg("str_rfind", args, 0)?;
    let sub = str_arg("str_rfind", args, 1)?;
    let pos = match s.rfind(sub) {
        Some(byte_pos) => s[..byte_pos].chars().count() as f64,
        None => -1.0,
    };
    Ok(Value::Num(pos))
}
