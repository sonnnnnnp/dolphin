use crate::diag::Diagnostic;
use crate::syntax::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
    /// 内側から順に（関数名, 呼び出し位置）
    pub trace: Vec<(String, Span)>,
}

impl RuntimeError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            trace: Vec::new(),
        }
    }
}

impl From<RuntimeError> for Diagnostic {
    fn from(e: RuntimeError) -> Self {
        let mut diag = Diagnostic::new(e.span, e.message);
        for (func, span) in e.trace {
            diag.notes.push(format!(
                "{func} の中（{}:{} から呼び出し）",
                span.line, span.col
            ));
        }
        diag
    }
}
