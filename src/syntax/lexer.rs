//! ソース文字列をトークン列に分割する（design.md 3 章）

use std::fmt;

use super::ast::Var;
use super::span::Span;
use crate::diag::Diagnostic;

/// 文字列リテラルの中身。`"Hi, @name!"` は Text("Hi, "), Var(@name), Text("!") になる。
/// `"@{a + 1}"` の `@{ }` は Interp になる（design.md 5.19）。
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Var(Var),
    /// `@{ 式 }` の式の字句（末尾は Eof）。式への解析は Parser が行う
    Interp(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Var(String),
    Ident(String),
    Number(f64),
    /// 整数リテラル `123i`（design.md 5.40）
    Int(i64),
    Str(Vec<StrPart>),

    // キーワード
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    True,
    False,
    Nil,

    // 記号
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    /// `...`（展開と可変長引数）
    Ellipsis,
    /// `?`（エラーを呼び出し元へ伝える）
    Question,
    Assign,
    /// `:=`
    Declare,
    Hash,

    // 演算子
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Bang,

    Newline,
    Eof,
}

/// エラーメッセージ用の表示
impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TokenKind::*;
        let s = match self {
            Var(name) => return write!(f, "`@{name}`"),
            Ident(name) => return write!(f, "`{name}`"),
            Number(n) => return write!(f, "`{n}`"),
            Int(n) => return write!(f, "`{n}i`"),
            Str(_) => "文字列",
            If => "`if`",
            Else => "`else`",
            While => "`while`",
            For => "`for`",
            In => "`in`",
            Break => "`break`",
            Continue => "`continue`",
            True => "`true`",
            False => "`false`",
            Nil => "`nil`",
            LBracket => "`[`",
            RBracket => "`]`",
            LParen => "`(`",
            RParen => "`)`",
            LBrace => "`{`",
            RBrace => "`}`",
            Comma => "`,`",
            Colon => "`:`",
            Dot => "`.`",
            Ellipsis => "`...`",
            Question => "`?`",
            Assign => "`=`",
            Declare => "`:=`",
            Hash => "`#`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            Percent => "`%`",
            Eq => "`==`",
            NotEq => "`!=`",
            Lt => "`<`",
            LtEq => "`<=`",
            Gt => "`>`",
            GtEq => "`>=`",
            AndAnd => "`&&`",
            OrOr => "`||`",
            Bang => "`!`",
            Newline => "改行",
            Eof => "ファイルの終わり",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// `chars[pos..]`（`@{` の `{` の次から）の `@{ }` の中身が終わる `}` の位置。
/// 中の `{ }` の入れ子と、中の文字列（その中の `@{ }` も）を飛ばす。閉じられていなければ None
pub fn interpolation_end(chars: &[char], mut pos: usize) -> Option<usize> {
    let mut depth = 0;
    while let Some(&c) = chars.get(pos) {
        match c {
            '"' => pos = string_end(chars, pos + 1)?,
            '{' => {
                depth += 1;
                pos += 1;
            }
            '}' if depth == 0 => return Some(pos),
            '}' => {
                depth -= 1;
                pos += 1;
            }
            _ => pos += 1,
        }
    }
    None
}

/// 開きの `"` の次から数えて、文字列を閉じる `"` の次の位置。閉じられていなければ None
pub fn string_end(chars: &[char], mut pos: usize) -> Option<usize> {
    while let Some(&c) = chars.get(pos) {
        match c {
            '\\' => pos += 2,
            '"' => return Some(pos + 1),
            '@' if chars.get(pos + 1) == Some(&'{') => {
                pos = interpolation_end(chars, pos + 2)? + 1;
            }
            _ => pos += 1,
        }
    }
    None
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, Diagnostic> {
    Lexer::new(src).run()
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    tokens: Vec<Token>,
    /// 開いている括弧のスタック。一番内側が `[` か `{` なら改行を無視する
    nesting: Vec<char>,
}

/// これ以下の整数は 64 ビット浮動小数点数で正確に表せる（2^53）
const MAX_EXACT: u64 = 1 << 53;

impl Lexer {
    fn new(src: &str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            nesting: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Vec<Token>, Diagnostic> {
        while let Some(c) = self.peek() {
            let start = self.span();
            match c {
                ' ' | '\t' | '\r' => {
                    self.bump();
                }
                '\n' => {
                    self.bump();
                    if !matches!(self.nesting.last(), Some('[' | '{')) {
                        self.push(TokenKind::Newline, start);
                    }
                }
                '/' if self.peek_next() == Some('/') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                '@' => {
                    self.bump();
                    let name = self.ident();
                    if name.is_empty() {
                        return self.error(start, "`@` の後に変数名がありません");
                    }
                    self.push(TokenKind::Var(name), start);
                }
                '$' => {
                    return self.error(start, "`$` は使えません（変数はすべて `@` で書きます）");
                }
                '"' => {
                    self.bump();
                    let parts = self.string(start)?;
                    self.push(TokenKind::Str(parts), start);
                }
                c if c.is_ascii_digit() => {
                    let kind = self.number(start)?;
                    self.push(kind, start);
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let word = self.ident();
                    if word == "r"
                        && matches!(self.peek(), Some('"' | '#'))
                        && let Some(parts) = self.raw_string(start)?
                    {
                        self.push(TokenKind::Str(parts), start);
                        continue;
                    }
                    let kind = match word.as_str() {
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "while" => TokenKind::While,
                        "for" => TokenKind::For,
                        "in" => TokenKind::In,
                        "nil" => TokenKind::Nil,
                        "break" => TokenKind::Break,
                        "continue" => TokenKind::Continue,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        _ => TokenKind::Ident(word),
                    };
                    self.push(kind, start);
                }
                _ => {
                    self.bump();
                    let kind = self.symbol(c, start)?;
                    self.push(kind, start);
                }
            }
        }
        let end = self.span();
        self.push(TokenKind::Eof, end);
        Ok(self.tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// 次の文字が `expected` なら読み進めて true を返す
    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }

    fn push(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(Token { kind, span });
    }

    fn error<T>(&self, span: Span, message: impl Into<String>) -> Result<T, Diagnostic> {
        Err(Diagnostic::new(span, message))
    }

    /// 英字か `_` で始まる識別子を読む。始まらなければ空文字列を返す
    fn ident(&mut self) -> String {
        let mut s = String::new();
        if !self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            return s;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        s
    }

    /// 数値リテラル（design.md 5.19）。`0xFF` `0b1010` `0o17`、桁区切りの `_`、指数 `1e9`。
    /// 末尾に `i` を付けると整数リテラル（design.md 5.40）
    fn number(&mut self, start: Span) -> Result<TokenKind, Diagnostic> {
        if self.peek() == Some('0')
            && let Some(radix) = match self.peek_next() {
                Some('x') => Some(16),
                Some('b') => Some(2),
                Some('o') => Some(8),
                _ => None,
            }
        {
            self.bump();
            self.bump();
            let digits = self.digits(radix, start)?;
            if digits.is_empty() {
                return self.error(start, "進数の接頭辞の後に数字がありません");
            }
            if self.int_suffix() {
                return i64::from_str_radix(&digits, radix)
                    .map(TokenKind::Int)
                    .or_else(|_| {
                        self.error(start, "整数が大きすぎます（9223372036854775807 まで）")
                    });
            }
            let n = u64::from_str_radix(&digits, radix)
                .ok()
                .filter(|n| *n <= MAX_EXACT)
                .ok_or_else(|| {
                    Diagnostic::new(
                        start,
                        "2^53 を超える整数は正確に表せません（整数にするなら末尾に `i` を付けます）",
                    )
                })?;
            return Ok(TokenKind::Number(n as f64));
        }
        let mut s = self.digits(10, start)?;
        let mut plain = true;
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            plain = false;
            s.push('.');
            self.bump();
            s.push_str(&self.digits(10, start)?);
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let sign = self.peek_next().filter(|c| matches!(c, '+' | '-'));
            let after = if sign.is_some() { 2 } else { 1 };
            if self
                .chars
                .get(self.pos + after)
                .is_some_and(char::is_ascii_digit)
            {
                plain = false;
                self.bump();
                s.push('e');
                if let Some(sign) = sign {
                    self.bump();
                    s.push(sign);
                }
                s.push_str(&self.digits(10, start)?);
            }
        }
        if self.int_suffix() {
            if !plain {
                return self.error(start, "小数や指数には整数の印 `i` を付けられません");
            }
            return s
                .parse::<i64>()
                .map(TokenKind::Int)
                .or_else(|_| self.error(start, "整数が大きすぎます（9223372036854775807 まで）"));
        }
        match s.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(TokenKind::Number(n)),
            _ => self.error(start, "数値が大きすぎます"),
        }
    }

    /// 数字の直後の整数の印 `i`（その後ろに英数字や `_` が続かないとき）を読む
    fn int_suffix(&mut self) -> bool {
        let is_suffix = self.peek() == Some('i')
            && !self
                .peek_next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_suffix {
            self.bump();
        }
        is_suffix
    }

    /// `radix` 進数の数字の並び。桁区切りの `_` は数字の間だけに置ける（読み捨てる）
    fn digits(&mut self, radix: u32, start: Span) -> Result<String, Diagnostic> {
        let mut s = String::new();
        let mut last_underscore = false;
        while let Some(c) = self.peek() {
            if c == '_' {
                // 先頭と連続は不可。直後が数字でなければ末尾の `_`
                let next_is_digit = self.peek_next().is_some_and(|n| n.is_digit(radix));
                if s.is_empty() || last_underscore || !next_is_digit {
                    return self.error(start, "桁区切りの `_` は数字と数字の間にだけ書けます");
                }
                last_underscore = true;
                self.bump();
            } else if c.is_digit(radix) {
                last_underscore = false;
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        Ok(s)
    }

    /// `@{ 式 }`。`@` を読んだ後（次が `{`）に呼ぶ。中の式を字句に分けて返す
    fn interpolation(&mut self, start: Span) -> Result<StrPart, Diagnostic> {
        self.bump(); // `{`
        let inner_start = self.span();
        let Some(end) = interpolation_end(&self.chars, self.pos) else {
            return Err(Diagnostic::new(start, "`@{` が閉じられていません").at_eof());
        };
        let mut sub = Lexer::new("");
        sub.chars = self.chars[self.pos..end].to_vec();
        sub.line = inner_start.line;
        sub.col = inner_start.col;
        let tokens = sub.run()?;
        while self.pos < end {
            self.bump();
        }
        self.bump(); // `}`
        Ok(StrPart::Interp(tokens))
    }

    /// 生の文字列 `r"…"` / `r#"…"#`。`r` を読んだ後、次が `"` か `#` のときに呼ぶ。
    /// 生の文字列でなければ（`#` の後に `"` がない）None
    fn raw_string(&mut self, start: Span) -> Result<Option<Vec<StrPart>>, Diagnostic> {
        let mut hashes = 0;
        while self.chars.get(self.pos + hashes) == Some(&'#') {
            hashes += 1;
        }
        if self.chars.get(self.pos + hashes) != Some(&'"') {
            return Ok(None);
        }
        for _ in 0..=hashes {
            self.bump();
        }
        let mut text = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(Diagnostic::new(start, "文字列が閉じられていません").at_eof());
            };
            if c == '"' && (0..hashes).all(|i| self.chars.get(self.pos + i) == Some(&'#')) {
                for _ in 0..hashes {
                    self.bump();
                }
                return Ok(Some(vec![StrPart::Text(text)]));
            }
            text.push(c);
        }
    }

    /// 開きの `"` を読んだ後に呼ぶ
    fn string(&mut self, start: Span) -> Result<Vec<StrPart>, Diagnostic> {
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            let esc_span = self.span();
            let Some(c) = self.bump() else {
                return Err(Diagnostic::new(start, "文字列が閉じられていません").at_eof());
            };
            match c {
                '"' => break,
                '\\' => match self.bump() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some('r') => text.push('\r'),
                    Some(c @ ('"' | '\\' | '@' | '$')) => text.push(c),
                    Some(other) => {
                        return self.error(esc_span, format!("不明なエスケープ `\\{other}`"));
                    }
                    None => {
                        return Err(Diagnostic::new(start, "文字列が閉じられていません").at_eof());
                    }
                },
                '@' if self.peek() == Some('{') => {
                    if !text.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut text)));
                    }
                    parts.push(self.interpolation(esc_span)?);
                }
                '@' => {
                    let name = self.ident();
                    if name.is_empty() {
                        // `@` の後に識別子が続かなければ、ただの文字
                        text.push(c);
                    } else {
                        if !text.is_empty() {
                            parts.push(StrPart::Text(std::mem::take(&mut text)));
                        }
                        parts.push(StrPart::Var(Var(name)));
                    }
                }
                _ => text.push(c),
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(StrPart::Text(text));
        }
        Ok(parts)
    }

    /// 記号・演算子。`c` は読み進めた後の 1 文字目
    fn symbol(&mut self, c: char, start: Span) -> Result<TokenKind, Diagnostic> {
        use TokenKind::*;
        let kind = match c {
            '[' | '(' | '{' => {
                self.nesting.push(c);
                match c {
                    '[' => LBracket,
                    '(' => LParen,
                    _ => LBrace,
                }
            }
            ']' | ')' | '}' => {
                // 対応のずれは Parser で検出する
                self.nesting.pop();
                match c {
                    ']' => RBracket,
                    ')' => RParen,
                    _ => RBrace,
                }
            }
            ',' => Comma,
            ':' if self.eat('=') => Declare,
            ':' => Colon,
            '.' if self.peek() == Some('.') && self.peek_next() == Some('.') => {
                self.bump();
                self.bump();
                Ellipsis
            }
            '.' => Dot,
            '?' => Question,
            '#' => Hash,
            '+' => Plus,
            '-' => Minus,
            '*' => Star,
            '/' => Slash,
            '%' => Percent,
            '=' if self.eat('=') => Eq,
            '=' => Assign,
            '!' if self.eat('=') => NotEq,
            '!' => Bang,
            '<' if self.eat('=') => LtEq,
            '<' => Lt,
            '>' if self.eat('=') => GtEq,
            '>' => Gt,
            '&' if self.eat('&') => AndAnd,
            '|' if self.eat('|') => OrOr,
            '&' | '|' => return self.error(start, format!("`{c}{c}` と書いてください")),
            _ => return self.error(start, format!("不明な文字 `{c}`")),
        };
        Ok(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::TokenKind::Var;
    use super::TokenKind::*;
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn assignment() {
        assert_eq!(
            kinds("@x = 1.5 + @y"),
            vec![
                Var("x".into()),
                Assign,
                Number(1.5),
                Plus,
                Var("y".into()),
                Eof,
            ]
        );
        assert_eq!(
            kinds("@x := 1"),
            vec![Var("x".into()), Declare, Number(1.0), Eof]
        );
    }

    #[test]
    fn for_in_nil() {
        assert_eq!(
            kinds("for @x in nil ("),
            vec![For, Var("x".into()), In, Nil, LParen, Eof]
        );
        // 識別子の一部なら、ただの名前
        assert_eq!(
            kinds("nilly[] input"),
            vec![
                Ident("nilly".into()),
                LBracket,
                RBracket,
                Ident("input".into()),
                Eof
            ]
        );
    }

    #[test]
    fn dollar_is_not_allowed() {
        let err = tokenize("$x = 1").unwrap_err();
        assert_eq!(err.span, Span { line: 1, col: 1 });
        assert!(err.message.contains("`$` は使えません"));
    }

    #[test]
    fn keywords_and_operators() {
        assert_eq!(
            kinds("if @a >= 1 && !true ("),
            vec![
                If,
                Var("a".into()),
                GtEq,
                Number(1.0),
                AndAnd,
                Bang,
                True,
                LParen,
                Eof,
            ]
        );
    }

    #[test]
    fn string_interpolation() {
        assert_eq!(
            kinds(r#""Hi, @name! $x costs $100 \@a @ end""#),
            vec![
                Str(vec![
                    StrPart::Text("Hi, ".into()),
                    StrPart::Var(crate::syntax::ast::Var("name".into())),
                    StrPart::Text("! $x costs $100 @a @ end".into()),
                ]),
                Eof,
            ]
        );
    }

    #[test]
    fn newlines_inside_brackets_are_ignored() {
        assert_eq!(
            kinds("f[1,\n2]\n(\n)"),
            vec![
                Ident("f".into()),
                LBracket,
                Number(1.0),
                Comma,
                Number(2.0),
                RBracket,
                Newline,
                LParen,
                Newline,
                RParen,
                Eof,
            ]
        );
    }

    #[test]
    fn comments() {
        assert_eq!(
            kinds("// head\n@a = 1 // tail"),
            vec![Newline, Var("a".into()), Assign, Number(1.0), Eof]
        );
    }

    #[test]
    fn error_has_position() {
        let err = tokenize("@a = 1\n@b = \"open").unwrap_err();
        assert_eq!(err.span, Span { line: 2, col: 6 });
    }

    #[test]
    fn int_literals() {
        use TokenKind::{Int, Number};
        let kinds = |src: &str| -> Vec<TokenKind> {
            tokenize(src).unwrap().into_iter().map(|t| t.kind).collect()
        };
        assert_eq!(
            kinds("1i 0xFFi 1_000i 0b11i 0o7i"),
            vec![Int(1), Int(255), Int(1000), Int(3), Int(7), TokenKind::Eof]
        );
        assert_eq!(
            kinds("9223372036854775807i"),
            vec![Int(i64::MAX), TokenKind::Eof]
        );
        // `i` の後ろに英数字が続くなら、整数の印ではない（数字のあとに名前が続く並びのまま）
        assert_eq!(
            kinds("5in"),
            vec![Number(5.0), TokenKind::In, TokenKind::Eof]
        );
        for bad in [
            "9223372036854775808i",
            "1.5i",
            "1e3i",
            "0xFFFFFFFFFFFFFFFFi",
        ] {
            assert!(tokenize(bad).is_err(), "{bad}");
        }
    }
}
