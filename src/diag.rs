//! エラー表示。Lexer・Parser・Resolver・実行時のエラーはすべてこの形で表示する

use std::fmt;
use std::rc::Rc;

use crate::syntax::span::Span;

/// import したファイルのパスとソース。エラーがそのファイルで起きたときの表示に使う
#[derive(Debug, PartialEq)]
pub struct Origin {
    pub path: String,
    pub src: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    /// 補足（呼び出し履歴など）。1 要素が 1 行
    pub notes: Vec<String>,
    /// 入力が途中で終わったせいのエラーか（閉じていない `(` や `"` など）。
    /// REPL はこれが true なら、エラーにせず次の行を読む
    pub unexpected_eof: bool,
    /// import したファイルで起きたエラーなら、そのファイル。None なら実行中の本体
    pub origin: Option<Rc<Origin>>,
}

impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            notes: Vec::new(),
            unexpected_eof: false,
            origin: None,
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
        // import したファイルで起きたエラーは、そのファイルの行を引用する
        let (path, src) = match &self.origin {
            Some(origin) => (origin.path.as_str(), origin.src.as_str()),
            None => (path, src),
        };
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
