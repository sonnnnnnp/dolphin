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

/// 呼び出し履歴が長いとき、先頭と末尾だけ表示する件数
const TRACE_SHOWN: usize = 5;

impl From<RuntimeError> for Diagnostic {
    fn from(e: RuntimeError) -> Self {
        let mut diag = Diagnostic::new(e.span, e.message);
        let n = e.trace.len();
        for (i, (func, span)) in e.trace.into_iter().enumerate() {
            if n > TRACE_SHOWN * 2 && (TRACE_SHOWN..n - TRACE_SHOWN).contains(&i) {
                if i == TRACE_SHOWN {
                    diag.notes
                        .push(format!("…（{} 件省略）", n - TRACE_SHOWN * 2));
                }
                continue;
            }
            diag.notes.push(format!(
                "{func} の中（{}:{} から呼び出し）",
                span.line, span.col
            ));
        }
        diag
    }
}
