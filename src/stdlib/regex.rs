//! `std:regex`: is_match, find, find_all, replace, replace_all, split, escape（design.md 5.20）
//!
//! `regex` クレートを使う（`regex` feature）。位置は文字単位で返す

use std::cell::RefCell;
use std::collections::HashMap;

use regex::{Captures, Regex};

use super::{error_value, expect_args, new_array, new_map, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

/// 使い回すコンパイル済みパターンの数
const CACHE_SIZE: usize = 64;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:regex",
        &[
            ("regex.is_match", is_match),
            ("regex.find", find),
            ("regex.find_all", find_all),
            ("regex.replace", |i, a| replace(i, a, "regex.replace", true)),
            ("regex.replace_all", |i, a| {
                replace(i, a, "regex.replace_all", false)
            }),
            ("regex.split", split),
            ("regex.escape", escape),
        ],
    );
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Regex>> = RefCell::default();
}

/// パターンをコンパイルする（同じパターンは使い回す）。間違いはエラー値
fn compile(pattern: &str) -> Result<Regex, Value> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(re) = cache.get(pattern) {
            return Ok(re.clone());
        }
        let re = Regex::new(pattern)
            .map_err(|e| error_value("parse", format!("正規表現が正しくありません: {e}")))?;
        if cache.len() >= CACHE_SIZE {
            cache.clear();
        }
        cache.insert(pattern.to_string(), re.clone());
        Ok(re)
    })
}

/// バイト位置を文字位置にする。前から順に呼ぶと、前回の続きから数える
struct CharIndex<'a> {
    text: &'a str,
    byte: usize,
    chars: usize,
}

impl<'a> CharIndex<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            chars: 0,
        }
    }

    fn at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.chars = 0;
        }
        self.chars += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.chars
    }
}

fn text_or_nil(m: Option<regex::Match<'_>>) -> Value {
    m.map_or(Value::Nil, |m| Value::Str(m.as_str().into()))
}

/// 一致マップ（design.md 5.20）
fn match_map(re: &Regex, caps: &Captures<'_>, index: &mut CharIndex<'_>) -> Value {
    let whole = caps.get(0).expect("グループ 0 は必ずある");
    let mut map = Map::default();
    map.insert("text".into(), Value::Str(whole.as_str().into()));
    map.insert("start".into(), Value::Num(index.at(whole.start()) as f64));
    map.insert("end".into(), Value::Num(index.at(whole.end()) as f64));
    let groups = (1..caps.len()).map(|i| text_or_nil(caps.get(i))).collect();
    map.insert("groups".into(), new_array(groups));
    let mut named = Map::default();
    for name in re.capture_names().flatten() {
        named.insert(name.into(), text_or_nil(caps.name(name)));
    }
    map.insert("named".into(), new_map(named));
    new_map(map)
}

/// `(パターン, 対象の文字列)` の引数 2 つ
fn pattern_and_text<'v>(
    name: &str,
    args: &'v [Value],
    n: usize,
) -> Result<(&'v str, &'v str), String> {
    expect_args(name, args, n)?;
    Ok((str_arg(name, args, 0)?, str_arg(name, args, 1)?))
}

fn is_match(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (pattern, text) = pattern_and_text("regex.is_match", args, 2)?;
    Ok(match compile(pattern) {
        Ok(re) => Value::Bool(re.is_match(text)),
        Err(e) => e,
    })
}

fn find(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (pattern, text) = pattern_and_text("regex.find", args, 2)?;
    let re = match compile(pattern) {
        Ok(re) => re,
        Err(e) => return Ok(e),
    };
    Ok(match re.captures(text) {
        Some(caps) => match_map(&re, &caps, &mut CharIndex::new(text)),
        None => Value::Nil,
    })
}

fn find_all(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (pattern, text) = pattern_and_text("regex.find_all", args, 2)?;
    let re = match compile(pattern) {
        Ok(re) => re,
        Err(e) => return Ok(e),
    };
    let mut index = CharIndex::new(text);
    let matches = re
        .captures_iter(text)
        .map(|caps| match_map(&re, &caps, &mut index))
        .collect();
    Ok(new_array(matches))
}

/// 置き換え。`replacement` が文字列なら `$1` `${name}` を展開し、関数なら一致マップを渡す
fn replace(
    interp: &mut Interpreter<'_>,
    args: &[Value],
    name: &str,
    first_only: bool,
) -> Result<Value, String> {
    let (pattern, text) = pattern_and_text(name, args, 3)?;
    let re = match compile(pattern) {
        Ok(re) => re,
        Err(e) => return Ok(e),
    };
    match &args[2] {
        Value::Str(rep) => {
            let out = if first_only {
                re.replace(text, rep.as_ref())
            } else {
                re.replace_all(text, rep.as_ref())
            };
            Ok(Value::Str(out.as_ref().into()))
        }
        f @ (Value::Func(_) | Value::Native(_)) => {
            let mut out = String::new();
            let mut last = 0;
            let mut index = CharIndex::new(text);
            for caps in re.captures_iter(text) {
                let whole = caps.get(0).expect("グループ 0 は必ずある");
                out.push_str(&text[last..whole.start()]);
                let value = interp.call_value(f, vec![match_map(&re, &caps, &mut index)])?;
                match value {
                    Value::Str(s) => out.push_str(&s),
                    other => {
                        return Err(format!(
                            "{name} の関数は文字列を返す必要があります（{}が返りました）",
                            other.type_name()
                        ));
                    }
                }
                last = whole.end();
                if first_only {
                    break;
                }
            }
            out.push_str(&text[last..]);
            Ok(Value::Str(out.into()))
        }
        other => Err(type_error(name, 2, "文字列か関数", other)),
    }
}

fn split(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (pattern, text) = pattern_and_text("regex.split", args, 2)?;
    let re = match compile(pattern) {
        Ok(re) => re,
        Err(e) => return Ok(e),
    };
    Ok(new_array(
        re.split(text).map(|s| Value::Str(s.into())).collect(),
    ))
}

fn escape(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("regex.escape", args, 1)?;
    Ok(Value::Str(
        regex::escape(str_arg("regex.escape", args, 0)?).into(),
    ))
}
