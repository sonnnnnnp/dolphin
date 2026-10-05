//! Dolphin 処理系の本体。CLI・テスト・将来の wasm 版から使う
//!
//! ソース → Lexer → Parser → AST → Resolver → Interpreter
//! 全体像は docs/architecture.md

pub mod diag;
pub mod runtime;
pub mod sema;
pub mod stdlib;
pub mod syntax;

use std::io::Write;

use diag::Diagnostic;
use runtime::interp::Interpreter;

/// ソースを解析して実行する。`log` の出力は `out` に書く
pub fn run_source(src: &str, out: &mut dyn Write) -> Result<(), Diagnostic> {
    let tokens = syntax::lexer::tokenize(src)?;
    let program = syntax::parser::parse(&tokens)?;
    sema::resolver::resolve(&program)?;

    let mut interp = Interpreter::new(out);
    stdlib::register(&mut interp);
    interp.run(&program)?;
    Ok(())
}
