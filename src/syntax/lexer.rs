//! ソース文字列をトークン列に分割する（design.md 3 章）

use std::fmt;

use super::ast::Var;
use super::span::Span;
use crate::diag::Diagnostic;

/// 文字列リテラルの中身。`"Hi, @name!"` は Text("Hi, "), Var(@name), Text("!") になる。
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Var(Var),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Var(String),
    Ident(String),
    Number(f64),
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
                    let n = self.number(start)?;
                    self.push(TokenKind::Number(n), start);
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let word = self.ident();
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

    /// 数値リテラル（design.md 5.19）。`0xFF` `0b1010` `0o17`、桁区切りの `_`、指数 `1e9`
    fn number(&mut self, start: Span) -> Result<f64, Diagnostic> {
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
            let n = u64::from_str_radix(&digits, radix)
                .ok()
                .filter(|n| *n <= MAX_EXACT)
                .ok_or_else(|| Diagnostic::new(start, "2^53 を超える整数は正確に表せません"))?;
            return Ok(n as f64);
        }
        let mut s = self.digits(10, start)?;
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
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
                self.bump();
                s.push('e');
                if let Some(sign) = sign {
                    self.bump();
                    s.push(sign);
                }
                s.push_str(&self.digits(10, start)?);
            }
        }
        match s.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(n),
            _ => self.error(start, "数値が大きすぎます"),
        }
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
}
