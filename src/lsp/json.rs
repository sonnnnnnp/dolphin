//! 言語サーバー用の小さな JSON（JSON-RPC の読み書きだけに使う）

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// 書かれた順を保つ
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn obj<const N: usize>(entries: [(&str, Json); N]) -> Json {
        Json::Obj(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    pub fn num(n: usize) -> Json {
        Json::Num(n as f64)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `a.b.c` のように辿る
    pub fn path(&self, path: &str) -> Option<&Json> {
        path.split('.').try_fold(self, |json, key| json.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
            _ => None,
        }
    }

    pub fn parse(src: &str) -> Result<Json, String> {
        let chars: Vec<char> = src.chars().collect();
        let mut p = Parser {
            chars: &chars,
            pos: 0,
        };
        let value = p.value(0)?;
        p.skip_ws();
        if p.pos != chars.len() {
            return Err("JSON の後ろに余分な文字があります".into());
        }
        Ok(value)
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) if n.is_finite() => {
                let _ = write!(out, "{n}");
            }
            Json::Num(_) => out.push_str("null"),
            Json::Str(s) => write_string(out, s),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
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
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.chars.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, word: &str) -> bool {
        let n = word.chars().count();
        let hit = self
            .chars
            .get(self.pos..self.pos + n)
            .is_some_and(|s| s.iter().copied().eq(word.chars()));
        if hit {
            self.pos += n;
        }
        hit
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("JSON の入れ子が深すぎます".into());
        }
        self.skip_ws();
        match self.chars.get(self.pos) {
            Some('{') => {
                self.pos += 1;
                let mut entries = Vec::new();
                self.skip_ws();
                if self.eat("}") {
                    return Ok(Json::Obj(entries));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if !self.eat(":") {
                        return Err("`:` がありません".into());
                    }
                    entries.push((key, self.value(depth + 1)?));
                    self.skip_ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Json::Obj(entries));
                    }
                    return Err("`,` か `}` がありません".into());
                }
            }
            Some('[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.eat("]") {
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Json::Arr(items));
                    }
                    return Err("`,` か `]` がありません".into());
                }
            }
            Some('"') => Ok(Json::Str(self.string()?)),
            Some(_) if self.eat("true") => Ok(Json::Bool(true)),
            Some(_) if self.eat("false") => Ok(Json::Bool(false)),
            Some(_) if self.eat("null") => Ok(Json::Null),
            Some(c) if c.is_ascii_digit() || *c == '-' => {
                let start = self.pos;
                while self
                    .chars
                    .get(self.pos)
                    .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
                {
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                text.parse()
                    .map(Json::Num)
                    .map_err(|_| format!("数値が読めません: {text}"))
            }
            _ => Err("JSON の値が読めません".into()),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return Err("文字列がありません".into());
        }
        let mut s = String::new();
        loop {
            let Some(&c) = self.chars.get(self.pos) else {
                return Err("文字列が閉じられていません".into());
            };
            self.pos += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let Some(&e) = self.chars.get(self.pos) else {
                        return Err("文字列が閉じられていません".into());
                    };
                    self.pos += 1;
                    match e {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        'b' => s.push('\u{8}'),
                        'f' => s.push('\u{c}'),
                        '/' | '\\' | '"' => s.push(e),
                        'u' => {
                            let mut unit = self.hex4()?;
                            // サロゲートペア
                            if (0xD800..0xDC00).contains(&unit) && self.eat("\\u") {
                                let low = self.hex4()?;
                                unit = 0x10000
                                    + ((unit - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            s.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(format!("不明なエスケープ `\\{e}`")),
                    }
                }
                c => s.push(c),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .chars
            .get(self.pos..self.pos + 4)
            .ok_or("`\\u` の後が足りません")?;
        let text: String = digits.iter().collect();
        self.pos += 4;
        u32::from_str_radix(&text, 16).map_err(|_| format!("`\\u{text}` が読めません"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let src = r#"{"a":[1,2.5,true,null],"b":"x\ny\"zあ","c":{}}"#;
        let json = Json::parse(src).unwrap();
        assert_eq!(json.path("b").unwrap().as_str(), Some("x\ny\"zあ"));
        assert_eq!(Json::parse(&json.to_string()).unwrap(), json);
    }

    #[test]
    fn surrogate_pair() {
        let json = Json::parse(r#""😀""#).unwrap();
        assert_eq!(json.as_str(), Some("😀"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Json::parse("{").is_err());
        assert!(Json::parse("1 2").is_err());
    }
}
