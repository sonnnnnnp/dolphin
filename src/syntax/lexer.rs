//! ソース文字列をトークン列に分割する（design.md 3 章）

use std::fmt;

use super::ast::Var;
use super::span::Span;
use crate::diag::Diagnostic;

/// 文字列リテラルの中身。`"Hi, @name!"` は Text("Hi, "), Var(@name), Text("!") になる。
/// `$x` も同じように展開する
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Var(Var),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Global(String),
    Local(String),
    Ident(String),
    Number(f64),
    Str(Vec<StrPart>),

    // キーワード
    If,
    Else,
    While,
    True,
    False,

    // 記号
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Assign,
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
            Global(name) => return write!(f, "`@{name}`"),
            Local(name) => return write!(f, "`${name}`"),
            Ident(name) => return write!(f, "`{name}`"),
            Number(n) => return write!(f, "`{n}`"),
            Str(_) => "文字列",
            If => "`if`",
            Else => "`else`",
            While => "`while`",
            True => "`true`",
            False => "`false`",
            LBracket => "`[`",
            RBracket => "`]`",
            LParen => "`(`",
            RParen => "`)`",
            LBrace => "`{`",
            RBrace => "`}`",
            Comma => "`,`",
            Assign => "`=`",
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
                '@' | '$' => {
                    self.bump();
                    let name = self.ident();
                    if name.is_empty() {
                        return self.error(start, format!("`{c}` の後に変数名がありません"));
                    }
                    let kind = if c == '@' {
                        TokenKind::Global(name)
                    } else {
                        TokenKind::Local(name)
                    };
                    self.push(kind, start);
                }
                '"' => {
                    self.bump();
                    let parts = self.string(start)?;
                    self.push(TokenKind::Str(parts), start);
                }
                c if c.is_ascii_digit() => {
                    let n = self.number();
                    self.push(TokenKind::Number(n), start);
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let word = self.ident();
                    let kind = match word.as_str() {
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "while" => TokenKind::While,
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

    fn number(&mut self) -> f64 {
        let mut s = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_ascii_digit()) {
            s.push(c);
            self.bump();
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            s.push('.');
            self.bump();
            while let Some(c) = self.peek().filter(|c| c.is_ascii_digit()) {
                s.push(c);
                self.bump();
            }
        }
        s.parse().expect("数字と . だけなので必ず数値になる")
    }

    /// 開きの `"` を読んだ後に呼ぶ
    fn string(&mut self, start: Span) -> Result<Vec<StrPart>, Diagnostic> {
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            let esc_span = self.span();
            let Some(c) = self.bump() else {
                return self.error(start, "文字列が閉じられていません");
            };
            match c {
                '"' => break,
                '\\' => match self.bump() {
                    Some('n') => text.push('\n'),
                    Some(c @ ('"' | '\\' | '@' | '$')) => text.push(c),
                    Some(other) => {
                        return self.error(esc_span, format!("不明なエスケープ `\\{other}`"));
                    }
                    None => return self.error(start, "文字列が閉じられていません"),
                },
                '@' | '$' => {
                    let name = self.ident();
                    if name.is_empty() {
                        // `$100` のように識別子が続かなければ、ただの文字
                        text.push(c);
                    } else {
                        if !text.is_empty() {
                            parts.push(StrPart::Text(std::mem::take(&mut text)));
                        }
                        let var = if c == '@' {
                            Var::Global(name)
                        } else {
                            Var::Local(name)
                        };
                        parts.push(StrPart::Var(var));
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
    use super::TokenKind::*;
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn assignment() {
        assert_eq!(
            kinds("@x = 1.5 + $y"),
            vec![
                Global("x".into()),
                Assign,
                Number(1.5),
                Plus,
                Local("y".into()),
                Eof,
            ]
        );
    }

    #[test]
    fn keywords_and_operators() {
        assert_eq!(
            kinds("if @a >= 1 && !true ("),
            vec![
                If,
                Global("a".into()),
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
            kinds(r#""Hi, @name! $x costs $100 \@a \$b""#),
            vec![
                Str(vec![
                    StrPart::Text("Hi, ".into()),
                    StrPart::Var(Var::Global("name".into())),
                    StrPart::Text("! ".into()),
                    StrPart::Var(Var::Local("x".into())),
                    StrPart::Text(" costs $100 @a $b".into()),
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
            vec![Newline, Global("a".into()), Assign, Number(1.0), Eof]
        );
    }

    #[test]
    fn error_has_position() {
        let err = tokenize("@a = 1\n@b = \"open").unwrap_err();
        assert_eq!(err.span, Span { line: 2, col: 6 });
    }
}
