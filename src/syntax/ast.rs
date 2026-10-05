//! 構文木（design.md 4 章）。すべての文と式が位置情報を持つ

use std::rc::Rc;

use super::lexer::StrPart;
use super::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// 評価器が関数表に登録するので Rc で共有する
    FuncDef(Rc<FuncDef>),
    /// `else if` は、if 文 1 つだけを含む else ブロックとして表す
    If {
        cond: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        cond: Expr,
        body: Block,
    },
    /// `# expr`
    Return(Option<Expr>),
    /// `@x = expr` / `@x[i] = expr`
    Assign {
        target: Var,
        index: Option<Expr>,
        value: Expr,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Var {
    Global(String),
    Local(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64),
    Str(Vec<StrPart>),
    Bool(bool),
    Var(Var),
    /// `name[args]`
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// `{a, b, c}`
    Array(Vec<Expr>),
    /// `target[index]`
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}
