//! `std:json`: parse, stringify, pretty（design.md 5.10）
//!
//! 外部クレートは使わず、RFC 8259 に沿って自前で読み書きする

use std::fmt::Write as _;
use std::rc::Rc;

use super::{error_value, expect_args, new_array, new_map, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:json",
        &[
            ("json.parse", parse),
            ("json.stringify", stringify),
            ("json.pretty", pretty),
        ],
    );
}

/// 入れ子の深さの上限。超えたら読み込みを諦める（再帰であふれないように）
const MAX_DEPTH: usize = 512;

fn parse(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "json.parse の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let src = str_arg("json.parse", args, 0)?;
    let mut ints = false;
    if let Some(options) = args.get(1) {
        let Value::Map(options) = options else {
            return Err(type_error("json.parse", 1, "マップ", options));
        };
        for (key, value) in options.borrow().iter() {
            match (&**key, value) {
                ("int", Value::Bool(b)) => ints = *b,
                ("int", other) => {
                    return Err(format!(
                        "json.parse のオプション `int` は真偽値です（{}が渡されました）",
                        other.type_name()
                    ));
                }
                _ => return Err(format!("json.parse のオプション `{key}` は知りません")),
            }
        }
    }
    Ok(match parse_str(src, ints) {
        Ok(value) => value,
        Err((pos, message)) => {
            let before = &src[..pos];
            let line = before.matches('\n').count() + 1;
            let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
            error_value(
                "parse",
                format!("JSON が正しくありません（{line} 行 {col} 列）: {message}"),
            )
        }
    })
}

fn stringify(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("json.stringify", args, 1)?;
    write_json("json.stringify", &args[0], None)
}

fn pretty(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("json.pretty", args, 1)?;
    write_json("json.pretty", &args[0], Some(2))
}

// ---- 読み込み ----

/// 失敗したときの (バイト位置, メッセージ)
type PErr = (usize, String);

/// `ints` が true なら、小数点も指数もない整数を `int` にする（design.md 5.40）
fn parse_str(src: &str, ints: bool) -> Result<Value, PErr> {
    let mut p = Parser {
        ints,
        src,
        bytes: src.as_bytes(),
        pos: 0,
        depth: 0,
    };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.pos < p.bytes.len() {
        return Err((p.pos, "値のあとに余分な文字があります".into()));
    }
    Ok(value)
}

struct Parser<'a> {
    ints: bool,
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, PErr> {
        Err((self.pos, message.into()))
    }

    fn expect(&mut self, byte: u8, what: &str) -> Result<(), PErr> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            self.err(format!("{what}が必要です"))
        }
    }

    fn value(&mut self) -> Result<Value, PErr> {
        match self.peek() {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => Ok(Value::Str(self.string()?.into())),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Nil),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.err("値が必要です"),
            None => self.err("値が途中で終わっています"),
        }
    }

    /// 配列とオブジェクトの入れ子の深さを数える
    fn nested(&mut self, f: fn(&mut Self) -> Result<Value, PErr>) -> Result<Value, PErr> {
        if self.depth >= MAX_DEPTH {
            return self.err(format!("入れ子が深すぎます（{MAX_DEPTH} 段まで）"));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, PErr> {
        if self.src[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            self.err("値が必要です")
        }
    }

    fn array(&mut self) -> Result<Value, PErr> {
        self.pos += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(new_array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(new_array(items));
                }
                _ => return self.err("`,` か `]` が必要です"),
            }
        }
    }

    fn object(&mut self) -> Result<Value, PErr> {
        self.pos += 1; // {
        let mut map = Map::default();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(new_map(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return self.err("キー（文字列）が必要です");
            }
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':', "`:`")?;
            self.skip_ws();
            let value = self.value()?;
            map.insert(key.into(), value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(new_map(map));
                }
                _ => return self.err("`,` か `}` が必要です"),
            }
        }
    }

    /// 開きの `"` の位置から読む
    fn string(&mut self) -> Result<String, PErr> {
        self.pos += 1;
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            let Some(b) = self.peek() else {
                return self.err("文字列が閉じられていません");
            };
            match b {
                b'"' => {
                    out.push_str(&self.src[run..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.src[run..self.pos]);
                    self.pos += 1;
                    out.push(self.escape()?);
                    run = self.pos;
                }
                0..=0x1f => return self.err("文字列の中に制御文字があります"),
                _ => self.pos += 1,
            }
        }
    }

    /// `\` の次の位置から読む
    fn escape(&mut self) -> Result<char, PErr> {
        let Some(b) = self.peek() else {
            return self.err("文字列が閉じられていません");
        };
        let start = self.pos;
        self.pos += 1;
        Ok(match b {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => self.unicode_escape(start)?,
            _ => return Err((start, "不明なエスケープです".into())),
        })
    }

    /// `\u` の次の位置から読む。サロゲートペアは 2 つ続けて 1 文字にする
    fn unicode_escape(&mut self, start: usize) -> Result<char, PErr> {
        let hi = self.hex4()?;
        match hi {
            0xD800..=0xDBFF => {
                if self.src[self.pos..].starts_with("\\u") {
                    self.pos += 2;
                    let lo = self.hex4()?;
                    if (0xDC00..=0xDFFF).contains(&lo) {
                        let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                        return Ok(char::from_u32(c).expect("サロゲートペアから作った値は有効"));
                    }
                }
                Err((start, "対になっていないサロゲートです".into()))
            }
            0xDC00..=0xDFFF => Err((start, "対になっていないサロゲートです".into())),
            _ => Ok(char::from_u32(hi).expect("サロゲート以外の 4 桁の値は有効")),
        }
    }

    fn hex4(&mut self) -> Result<u32, PErr> {
        let digits = self.src.get(self.pos..self.pos + 4);
        match digits.and_then(|d| u32::from_str_radix(d, 16).ok()) {
            // `from_str_radix` は先頭の `+` も許すので、16 進数の桁だけか確かめる
            Some(n) if digits.is_some_and(|d| d.bytes().all(|b| b.is_ascii_hexdigit())) => {
                self.pos += 4;
                Ok(n)
            }
            _ => self.err("`\\u` の後に 16 進数 4 桁が必要です"),
        }
    }

    fn number(&mut self) -> Result<Value, PErr> {
        let start = self.pos;
        let mut plain = true;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return self.err("数字が必要です"),
        }
        if self.peek() == Some(b'.') {
            plain = false;
            self.pos += 1;
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return self.err("小数点の後に数字が必要です");
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            plain = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return self.err("指数の後に数字が必要です");
            }
            self.digits();
        }
        if self.ints && plain {
            return match self.src[start..self.pos].parse::<i64>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => Err((start, "整数が大きすぎます（int は 64 ビットまで）".into())),
            };
        }
        match self.src[start..self.pos].parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Value::Num(n)),
            _ => Err((start, "数値が大きすぎます".into())),
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
    }
}

// ---- 書き出し ----

/// `indent` が Some なら、その幅で整形する
fn write_json(name: &str, value: &Value, indent: Option<usize>) -> Result<Value, String> {
    Ok(Value::Str(to_json_string(name, value, indent)?.into()))
}

/// 値を JSON の文字列にする。`name` はエラーメッセージに出す関数名。
/// JSON にできない値は、プログラムの間違いとしてメッセージを返す
pub fn to_json_string(name: &str, value: &Value, indent: Option<usize>) -> Result<String, String> {
    let mut out = String::new();
    let mut seen = Vec::new();
    write_value(name, &mut out, value, indent, 0, &mut seen)?;
    Ok(out)
}

fn write_value(
    name: &str,
    out: &mut String,
    value: &Value,
    indent: Option<usize>,
    level: usize,
    seen: &mut Vec<*const ()>,
) -> Result<(), String> {
    match value {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => write!(out, "{b}").expect("String への書き込みは失敗しない"),
        Value::Num(n) if n.is_finite() => {
            write!(out, "{n}").expect("String への書き込みは失敗しない")
        }
        Value::Num(n) => return Err(format!("{name}: {n} は JSON にできません")),
        Value::Int(n) => write!(out, "{n}").expect("String への書き込みは失敗しない"),
        Value::Str(s) => write_string(out, s),
        Value::Array(items) => {
            let ptr = Rc::as_ptr(items).cast::<()>();
            enter(name, seen, ptr)?;
            let items = items.borrow();
            if items.is_empty() {
                out.push_str("[]");
            } else {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    separator(out, i, indent, level + 1);
                    write_value(name, out, item, indent, level + 1, seen)?;
                }
                close(out, ']', indent, level);
            }
            seen.pop();
        }
        Value::Map(map) => {
            let ptr = Rc::as_ptr(map).cast::<()>();
            enter(name, seen, ptr)?;
            let map = map.borrow();
            if map.is_empty() {
                out.push_str("{}");
            } else {
                out.push('{');
                for (i, (key, item)) in map.iter().enumerate() {
                    separator(out, i, indent, level + 1);
                    write_string(out, key);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    write_value(name, out, item, indent, level + 1, seen)?;
                }
                close(out, '}', indent, level);
            }
            seen.pop();
        }
        other => {
            return Err(format!("{name}: {}は JSON にできません", other.type_name()));
        }
    }
    Ok(())
}

/// 自分自身を含む配列・マップは、無限に書き続けないようエラーにする
fn enter(name: &str, seen: &mut Vec<*const ()>, ptr: *const ()) -> Result<(), String> {
    if seen.contains(&ptr) {
        return Err(format!("{name}: 循環した配列やマップは JSON にできません"));
    }
    seen.push(ptr);
    Ok(())
}

/// 要素の前の `,` と改行・インデント
fn separator(out: &mut String, i: usize, indent: Option<usize>, level: usize) {
    if i > 0 {
        out.push(',');
    }
    if let Some(width) = indent {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', width * level));
    }
}

fn close(out: &mut String, bracket: char, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', width * level));
    }
    out.push(bracket);
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{:04x}", c as u32).expect("String への書き込みは失敗しない");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn roundtrip(src: &str) -> String {
        let value = parse_str(src, false).unwrap();
        let Value::Str(s) = write_json("t", &value, None).unwrap() else {
            unreachable!()
        };
        s.to_string()
    }

    fn parse_err(src: &str) -> (usize, String) {
        parse_str(src, false).unwrap_err()
    }

    #[test]
    fn scalars() {
        assert_eq!(roundtrip("null"), "null");
        assert_eq!(roundtrip(" true "), "true");
        assert_eq!(roundtrip("-0.5e1"), "-5");
        assert_eq!(roundtrip("12"), "12");
        assert_eq!(roundtrip("1.25"), "1.25");
        assert_eq!(roundtrip(r#""a\nb""#), r#""a\nb""#);
    }

    #[test]
    fn structures_keep_order() {
        assert_eq!(
            roundtrip(r#" { "b": [1, 2, {"c": null}], "a": {} , "z": [] } "#),
            r#"{"b":[1,2,{"c":null}],"a":{},"z":[]}"#
        );
        // 重複したキーは、あとのものが勝つ（位置は最初のまま）
        assert_eq!(roundtrip(r#"{"a":1,"b":2,"a":3}"#), r#"{"a":3,"b":2}"#);
    }

    #[test]
    fn string_escapes() {
        let v = parse_str(r#""Aé😀\/\t""#, false).unwrap();
        assert_eq!(v.to_string(), "Aé😀/\t");
        // 出力: 制御文字は \u00XX、非 ASCII はそのまま
        let out = write_json("t", &Value::Str("é\u{1}\"\\".into()), None).unwrap();
        assert_eq!(out.to_string(), "\"é\\u0001\\\"\\\\\"");
    }

    #[test]
    fn errors_have_positions() {
        assert_eq!(parse_err(""), (0, "値が途中で終わっています".into()));
        assert_eq!(parse_err("[1, 2,]").0, 6);
        assert_eq!(parse_err("{\"a\" 1}").1, "`:`が必要です");
        assert_eq!(parse_err("[1] x").1, "値のあとに余分な文字があります");
        assert_eq!(parse_err("01").1, "値のあとに余分な文字があります");
        assert_eq!(parse_err("1.").1, "小数点の後に数字が必要です");
        assert_eq!(parse_err("1e").1, "指数の後に数字が必要です");
        assert_eq!(parse_err("1e999").1, "数値が大きすぎます");
        assert_eq!(parse_err("'a'").1, "値が必要です");
        assert_eq!(parse_err("\"a").1, "文字列が閉じられていません");
        assert_eq!(parse_err("\"a\nb\"").1, "文字列の中に制御文字があります");
        assert_eq!(parse_err(r#""\x""#).1, "不明なエスケープです");
        assert_eq!(parse_err(r#""\ud800""#).1, "対になっていないサロゲートです");
        assert_eq!(
            parse_err(r#""\u12G4""#).1,
            "`\\u` の後に 16 進数 4 桁が必要です"
        );
        assert_eq!(parse_err("[1, 2").1, "`,` か `]` が必要です");
        assert_eq!(parse_err("tru").1, "値が必要です");
    }

    #[test]
    fn depth_limit() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse_str(&ok, false).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(parse_err(&deep).1.starts_with("入れ子が深すぎます"));
    }

    #[test]
    fn pretty_output() {
        let value = parse_str(r#"{"a":[1,{"b":[]}],"c":{}}"#, false).unwrap();
        let out = write_json("t", &value, Some(2)).unwrap().to_string();
        assert_eq!(
            out,
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": []\n    }\n  ],\n  \"c\": {}\n}"
        );
    }

    #[test]
    fn unrepresentable_values() {
        assert!(write_json("t", &Value::Num(f64::NAN), None).is_err());
        assert!(write_json("t", &Value::Num(f64::INFINITY), None).is_err());
        assert!(write_json("t", &Value::error("x", "y"), None).is_err());
        // 循環
        let array = Rc::new(RefCell::new(vec![Value::Num(1.0)]));
        array.borrow_mut().push(Value::Array(Rc::clone(&array)));
        let e = write_json("t", &Value::Array(array), None).unwrap_err();
        assert!(e.contains("循環"));
        // 同じ配列を 2 回含んでも循環ではない
        let inner = Value::Array(Rc::new(RefCell::new(vec![Value::Num(1.0)])));
        let outer = Value::Array(Rc::new(RefCell::new(vec![inner.clone(), inner])));
        assert_eq!(
            write_json("t", &outer, None).unwrap().to_string(),
            "[[1],[1]]"
        );
    }

    #[test]
    fn ints_option_and_writing() {
        let v = parse_str("[1, 1.5, 1e2, -0, 9223372036854775807]", true).unwrap();
        assert_eq!(v.to_string(), "{1, 1.5, 100, 0, 9223372036854775807}");
        let Value::Array(items) = &v else { panic!() };
        let kinds: Vec<_> = items.borrow().iter().map(Value::type_name).collect();
        assert_eq!(kinds, ["整数", "数値", "数値", "整数", "整数"]);
        assert!(parse_str("9223372036854775808", true).is_err());
        // 既定では、大きい整数も数値のまま
        assert!(matches!(
            parse_str("9223372036854775808", false),
            Ok(Value::Num(_))
        ));
        assert_eq!(
            to_json_string("t", &Value::Int(i64::MIN), None).unwrap(),
            "-9223372036854775808"
        );
    }
}
