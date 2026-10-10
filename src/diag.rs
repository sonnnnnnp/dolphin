//! エラー表示。Lexer・Parser・Resolver・実行時のエラーはすべてこの形で表示する

use std::fmt;

use crate::syntax::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    /// 補足（呼び出し履歴など）。1 要素が 1 行
    pub notes: Vec<String>,
    /// 入力が途中で終わったせいのエラーか（閉じていない `(` や `"` など）。
    /// REPL はこれが true なら、エラーにせず次の行を読む
    pub unexpected_eof: bool,
}

impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            notes: Vec::new(),
            unexpected_eof: false,
        }
    }

    pub fn at_eof(mut self) -> Self {
        self.unexpected_eof = true;
        self
    }

    /// 該当行を引用し、列の位置に `^` を付けて表示する
    ///
    /// ```text
    /// main.dol:2:6: エラー: 文字列が閉じられていません
    ///   |
    /// 2 | @b = "open
    ///   |      ^
    /// ```
    pub fn render(&self, path: &str, src: &str) -> String {
        let Span { line, col } = self.span;
        let mut s = format!("{path}:{line}:{col}: エラー: {}\n", self.message);
        if let Some(text) = src.lines().nth(line - 1) {
            let num = line.to_string();
            let pad = " ".repeat(num.len());
            s += &format!(
                "{pad} |\n{num} | {text}\n{pad} | {}^\n",
                " ".repeat(col - 1)
            );
        }
        for note in &self.notes {
            s += &format!("  = {note}\n");
        }
        s
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}
