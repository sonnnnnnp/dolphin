use std::rc::Rc;

use crate::diag::{Diagnostic, Origin};
use crate::syntax::span::Span;

/// 呼び出し履歴の 1 件
#[derive(Debug, Clone, PartialEq)]
pub struct TraceEntry {
    /// 「f の中」「util.dol の読み込み中」
    pub what: String,
    /// 呼び出し位置
    pub span: Span,
    /// 呼び出し位置があるファイル。実行中の本体なら None
    pub file: Option<Rc<str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
    /// 内側から順に
    pub trace: Vec<TraceEntry>,
    /// import したファイルで起きたエラーなら、そのファイル
    pub origin: Option<Rc<Origin>>,
}

impl RuntimeError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            trace: Vec::new(),
            origin: None,
        }
    }
}

/// 呼び出し履歴が長いとき、先頭と末尾だけ表示する件数
const TRACE_SHOWN: usize = 5;

impl From<RuntimeError> for Diagnostic {
    fn from(e: RuntimeError) -> Self {
        let mut diag = Diagnostic::new(e.span, e.message);
        diag.origin = e.origin;
        let n = e.trace.len();
        for (i, entry) in e.trace.into_iter().enumerate() {
            if n > TRACE_SHOWN * 2 && (TRACE_SHOWN..n - TRACE_SHOWN).contains(&i) {
                if i == TRACE_SHOWN {
                    diag.notes
                        .push(format!("…（{} 件省略）", n - TRACE_SHOWN * 2));
                }
                continue;
            }
            let Span { line, col } = entry.span;
            let file = entry.file.map(|f| format!("{f}:")).unwrap_or_default();
            diag.notes
                .push(format!("{}（{file}{line}:{col} から呼び出し）", entry.what));
        }
        diag
    }
}
