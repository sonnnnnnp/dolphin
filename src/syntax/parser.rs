//! トークン列から AST を作る。design.md 4 章の EBNF を再帰下降で実装する

use super::ast::Program;
use super::lexer::Token;
use crate::diag::Diagnostic;

pub fn parse(_tokens: &[Token]) -> Result<Program, Diagnostic> {
    todo!("段階 1: design.md 4 章の文法を実装する")
}
