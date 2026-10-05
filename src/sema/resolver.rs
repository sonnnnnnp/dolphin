//! 実行前の検査（design.md 5 章）
//!
//! - 全関数を事前に集め、未定義の関数呼び出しを検出する
//! - 関数の外で `$x` を使っていないか検査する
//! - 呼び出しの引数の数を検査する

use crate::diag::Diagnostic;
use crate::syntax::ast::Program;

pub fn resolve(_program: &Program) -> Result<(), Diagnostic> {
    todo!("段階 1: スコープと関数呼び出しを検査する")
}
