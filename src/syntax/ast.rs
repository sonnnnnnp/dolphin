//! 構文木（design.md 4 章）。すべての文と式が位置情報を持つ

use std::fmt;
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
    /// `while` を抜ける
    Break,
    /// `while` の次の繰り返しへ進む
    Continue,
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
    /// `{"k": v, ...}`。空は `{:}`
    Map(Vec<(Expr, Expr)>),
    /// `@u.name`。モジュールの `@` 変数を読む
    Member {
        target: Box<Expr>,
        name: String,
    },
    /// `@u.name[args]`。モジュールの関数を呼ぶ
    MemberCall {
        target: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
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

// ---- S 式での表示（`dolphin --ast` とテストで使う） ----
//
// @x = 1 + 2      → (= @x (+ 1 2))
// @a[0] = 1       → (= ([] @a 0) 1)
// add[$a, $b] ( … ) → (def add ($a $b) (…))
// if c ( … ) else ( … ) → (if c (…) (…))

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for stmt in &self.stmts {
            writeln!(f, "{stmt}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            StmtKind::FuncDef(def) => {
                write!(f, "(def {} (", def.name)?;
                write_seq(f, def.params.iter().map(|p| format!("${p}")))?;
                write!(f, ") {})", BlockDisplay(&def.body))
            }
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                write!(f, "(if {cond} {}", BlockDisplay(then_block))?;
                if let Some(else_block) = else_block {
                    write!(f, " {}", BlockDisplay(else_block))?;
                }
                write!(f, ")")
            }
            StmtKind::While { cond, body } => write!(f, "(while {cond} {})", BlockDisplay(body)),
            StmtKind::Return(Some(value)) => write!(f, "(# {value})"),
            StmtKind::Return(None) => write!(f, "(#)"),
            StmtKind::Break => write!(f, "(break)"),
            StmtKind::Continue => write!(f, "(continue)"),
            StmtKind::Assign {
                target,
                index: Some(index),
                value,
            } => write!(f, "(= ([] {target} {index}) {value})"),
            StmtKind::Assign {
                target,
                index: None,
                value,
            } => write!(f, "(= {target} {value})"),
            StmtKind::Expr(expr) => write!(f, "{expr}"),
        }
    }
}

struct BlockDisplay<'a>(&'a [Stmt]);

impl fmt::Display for BlockDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        write_seq(f, self.0)?;
        write!(f, ")")
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExprKind::Number(n) => write!(f, "{n}"),
            ExprKind::Str(parts) => {
                write!(f, "\"")?;
                for part in parts {
                    match part {
                        StrPart::Text(text) => {
                            let escaped = text
                                .escape_debug()
                                .to_string()
                                .replace('@', "\\@")
                                .replace('$', "\\$");
                            write!(f, "{escaped}")?;
                        }
                        StrPart::Var(var) => write!(f, "{var}")?,
                    }
                }
                write!(f, "\"")
            }
            ExprKind::Bool(b) => write!(f, "{b}"),
            ExprKind::Var(var) => write!(f, "{var}"),
            ExprKind::Call { name, args } => {
                write!(f, "({name}")?;
                for arg in args {
                    write!(f, " {arg}")?;
                }
                write!(f, ")")
            }
            ExprKind::Array(items) => {
                write!(f, "{{")?;
                write_seq(f, items)?;
                write!(f, "}}")
            }
            ExprKind::Map(entries) if entries.is_empty() => write!(f, "{{:}}"),
            ExprKind::Map(entries) => {
                write!(f, "{{")?;
                write_seq(f, entries.iter().map(|(k, v)| format!("{k}: {v}")))?;
                write!(f, "}}")
            }
            ExprKind::Member { target, name } => write!(f, "(. {target} {name})"),
            ExprKind::MemberCall { target, name, args } => {
                write!(f, "((. {target} {name})")?;
                for arg in args {
                    write!(f, " {arg}")?;
                }
                write!(f, ")")
            }
            ExprKind::Index { target, index } => write!(f, "([] {target} {index})"),
            ExprKind::Unary { op, operand } => write!(f, "({op} {operand})"),
            ExprKind::Binary { op, lhs, rhs } => write!(f, "({op} {lhs} {rhs})"),
        }
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Var::Global(name) => write!(f, "@{name}"),
            Var::Local(name) => write!(f, "${name}"),
        }
    }
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        })
    }
}

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::LtEq => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::GtEq => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
        })
    }
}

/// 空白区切りで並べる
fn write_seq<T: fmt::Display>(
    f: &mut fmt::Formatter<'_>,
    items: impl IntoIterator<Item = T>,
) -> fmt::Result {
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            write!(f, " ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}
