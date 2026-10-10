//! 実行前の検査（design.md 5 章）
//!
//! - 関数はトップレベルにだけ定義でき、名前は重複せず、組み込み関数とも重ならない
//! - 呼び出す関数が定義されていて、引数の数が合っている（組み込み関数は個数を実行時に検査）
//! - `$x` と `#` は関数の中でだけ使える
//! - `break` と `continue` は `while` の中でだけ使える
//! - 読む `$x` は、引数か、関数内のどこかで代入されている
//!
//! `@x` の未定義は、関数の中から代入されることもあるので実行時に検査する

use std::collections::{HashMap, HashSet};

use crate::diag::Diagnostic;
use crate::syntax::ast::{Expr, ExprKind, FuncDef, Program, Stmt, StmtKind, Var};
use crate::syntax::lexer::StrPart;
use crate::syntax::span::Span;

type RResult = Result<(), Diagnostic>;

/// プログラムの外ですでに定義されている関数
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callee {
    /// 組み込み関数。引数の数は実行時に検査する
    Native,
    /// 以前の入力で定義した関数（REPL）
    User { arity: usize },
}

/// `lookup` は、プログラムの外で定義済みの関数を名前から引く。
/// プログラム内で同じ名前を定義した場合はそちらが優先（REPL での再定義）
pub fn resolve(program: &Program, lookup: impl Fn(&str) -> Option<Callee>) -> RResult {
    let mut functions = HashMap::new();
    for stmt in &program.stmts {
        if let StmtKind::FuncDef(def) = &stmt.kind {
            if lookup(&def.name) == Some(Callee::Native) {
                return Err(Diagnostic::new(
                    def.span,
                    format!(
                        "組み込み関数 `{}` と同じ名前の関数は定義できません",
                        def.name
                    ),
                ));
            }
            if functions
                .insert(def.name.as_str(), def.params.len())
                .is_some()
            {
                return Err(Diagnostic::new(
                    def.span,
                    format!("関数 `{}` はすでに定義されています", def.name),
                ));
            }
        }
    }

    let resolver = Resolver {
        functions,
        lookup: &lookup,
    };
    for stmt in &program.stmts {
        match &stmt.kind {
            StmtKind::FuncDef(def) => resolver.func_def(def)?,
            _ => resolver.stmt(stmt, None, false)?,
        }
    }
    Ok(())
}

/// 関数の中なら、その関数のローカル変数の集合。トップレベルなら None
type Locals<'a, 'b> = Option<&'b HashSet<&'a str>>;

struct Resolver<'a> {
    /// 関数名 → 引数の数
    functions: HashMap<&'a str, usize>,
    lookup: &'a dyn Fn(&str) -> Option<Callee>,
}

impl<'a> Resolver<'a> {
    fn func_def(&self, def: &'a FuncDef) -> RResult {
        let mut locals = HashSet::new();
        for param in &def.params {
            if !locals.insert(param.as_str()) {
                return Err(Diagnostic::new(
                    def.span,
                    format!("引数 `${param}` が重複しています"),
                ));
            }
        }
        collect_locals(&def.body, &mut locals);
        self.block(&def.body, Some(&locals), false)
    }

    /// `in_loop` は `while` の中か（`break` / `continue` を使えるか）
    fn block(&self, block: &'a [Stmt], locals: Locals<'a, '_>, in_loop: bool) -> RResult {
        block
            .iter()
            .try_for_each(|stmt| self.stmt(stmt, locals, in_loop))
    }

    fn stmt(&self, stmt: &'a Stmt, locals: Locals<'a, '_>, in_loop: bool) -> RResult {
        match &stmt.kind {
            StmtKind::FuncDef(def) => Err(Diagnostic::new(
                def.span,
                "関数はトップレベルでのみ定義できます",
            )),
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                self.expr(cond, locals)?;
                self.block(then_block, locals, in_loop)?;
                match else_block {
                    Some(block) => self.block(block, locals, in_loop),
                    None => Ok(()),
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, locals)?;
                self.block(body, locals, true)
            }
            StmtKind::Break | StmtKind::Continue if !in_loop => {
                let keyword = if let StmtKind::Break = stmt.kind {
                    "break"
                } else {
                    "continue"
                };
                Err(Diagnostic::new(
                    stmt.span,
                    format!("`{keyword}` は while の中でのみ使えます"),
                ))
            }
            StmtKind::Break | StmtKind::Continue => Ok(()),
            StmtKind::Return(value) => {
                if locals.is_none() {
                    return Err(Diagnostic::new(stmt.span, "`#` は関数の中でのみ使えます"));
                }
                match value {
                    Some(value) => self.expr(value, locals),
                    None => Ok(()),
                }
            }
            StmtKind::Assign {
                target,
                index,
                value,
            } => {
                match index {
                    // `@a[i] = v` は @a を読む
                    Some(index) => {
                        self.var(target, stmt.span, locals)?;
                        self.expr(index, locals)?;
                    }
                    None => {
                        if let Var::Local(_) = target {
                            local_allowed(target, stmt.span, locals)?;
                        }
                    }
                }
                self.expr(value, locals)
            }
            StmtKind::Expr(expr) => self.expr(expr, locals),
        }
    }

    fn expr(&self, expr: &'a Expr, locals: Locals<'a, '_>) -> RResult {
        match &expr.kind {
            ExprKind::Number(_) | ExprKind::Bool(_) => Ok(()),
            ExprKind::Str(parts) => parts.iter().try_for_each(|part| match part {
                StrPart::Var(var) => self.var(var, expr.span, locals),
                StrPart::Text(_) => Ok(()),
            }),
            ExprKind::Var(var) => self.var(var, expr.span, locals),
            ExprKind::Call { name, args } => {
                let arity = match self.functions.get(name.as_str()) {
                    Some(&n) => Some(n),
                    None => match (self.lookup)(name) {
                        Some(Callee::User { arity }) => Some(arity),
                        Some(Callee::Native) => None,
                        None => {
                            return Err(Diagnostic::new(
                                expr.span,
                                format!("未定義の関数 `{name}`"),
                            ));
                        }
                    },
                };
                // 組み込み関数（arity が None）の引数の数は実行時に検査する
                if let Some(n) = arity
                    && args.len() != n
                {
                    return Err(Diagnostic::new(
                        expr.span,
                        format!(
                            "`{name}` の引数は {n} 個です（{} 個渡されました）",
                            args.len()
                        ),
                    ));
                }
                args.iter().try_for_each(|arg| self.expr(arg, locals))
            }
            ExprKind::Array(items) => items.iter().try_for_each(|item| self.expr(item, locals)),
            ExprKind::Map(entries) => entries.iter().try_for_each(|(key, value)| {
                self.expr(key, locals)?;
                self.expr(value, locals)
            }),
            ExprKind::Index { target, index } => {
                self.expr(target, locals)?;
                self.expr(index, locals)
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, locals),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, locals)?;
                self.expr(rhs, locals)
            }
        }
    }

    /// 変数を読めるか
    fn var(&self, var: &Var, span: Span, locals: Locals<'a, '_>) -> RResult {
        let Var::Local(name) = var else {
            return Ok(());
        };
        let locals = local_allowed(var, span, locals)?;
        if locals.contains(name.as_str()) {
            Ok(())
        } else {
            Err(Diagnostic::new(span, format!("未定義の変数 `${name}`")))
        }
    }
}

/// `$x` を使える場所（関数の中）か
fn local_allowed<'a, 'b>(
    var: &Var,
    span: Span,
    locals: Locals<'a, 'b>,
) -> Result<&'b HashSet<&'a str>, Diagnostic> {
    locals.ok_or_else(|| Diagnostic::new(span, format!("`{var}` は関数の中でのみ使えます")))
}

/// ブロック内（入れ子も含む）で代入されている `$x` を集める
fn collect_locals<'a>(block: &'a [Stmt], out: &mut HashSet<&'a str>) {
    for stmt in block {
        match &stmt.kind {
            StmtKind::Assign {
                target: Var::Local(name),
                ..
            } => {
                out.insert(name);
            }
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                collect_locals(then_block, out);
                if let Some(block) = else_block {
                    collect_locals(block, out);
                }
            }
            StmtKind::While { body, .. } => collect_locals(body, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{lexer::tokenize, parser::parse};

    fn check(src: &str) -> Result<(), String> {
        let program = parse(&tokenize(src).unwrap()).unwrap();
        resolve(&program, |name| (name == "log").then_some(Callee::Native)).map_err(|e| e.message)
    }

    #[test]
    fn valid_program() {
        let src = "\
log[add[1, 2]]
add[$a, $b] (
    if $a > 0 (
        $sum = $a + $b
    )
    # $sum
)";
        assert_eq!(check(src), Ok(()));
    }

    #[test]
    fn undefined_function() {
        assert_eq!(check("foo[1]"), Err("未定義の関数 `foo`".into()));
    }

    #[test]
    fn wrong_arity() {
        assert_eq!(
            check("f[$a] (\n)\nf[1, 2]"),
            Err("`f` の引数は 1 個です（2 個渡されました）".into())
        );
    }

    #[test]
    fn duplicate_function() {
        assert_eq!(
            check("f[] (\n)\nf[] (\n)"),
            Err("関数 `f` はすでに定義されています".into())
        );
    }

    #[test]
    fn shadowing_native() {
        assert_eq!(
            check("log[$x] (\n)"),
            Err("組み込み関数 `log` と同じ名前の関数は定義できません".into())
        );
    }

    #[test]
    fn nested_function() {
        assert_eq!(
            check("if true (\n    f[] (\n    )\n)"),
            Err("関数はトップレベルでのみ定義できます".into())
        );
    }

    #[test]
    fn local_outside_function() {
        assert_eq!(check("$x = 1"), Err("`$x` は関数の中でのみ使えます".into()));
        assert_eq!(
            check("log[$x]"),
            Err("`$x` は関数の中でのみ使えます".into())
        );
    }

    #[test]
    fn undefined_local() {
        assert_eq!(
            check("f[$a] (\n    # $b\n)"),
            Err("未定義の変数 `$b`".into())
        );
    }

    #[test]
    fn local_in_string() {
        assert_eq!(
            check("log[\"$x\"]"),
            Err("`$x` は関数の中でのみ使えます".into())
        );
        assert_eq!(
            check("f[$a] (\n    log[\"$a $b\"]\n)"),
            Err("未定義の変数 `$b`".into())
        );
    }

    #[test]
    fn break_outside_loop() {
        assert_eq!(
            check("break"),
            Err("`break` は while の中でのみ使えます".into())
        );
        assert_eq!(
            check("if true (\n    continue\n)"),
            Err("`continue` は while の中でのみ使えます".into())
        );
        // 関数は while の外にあるので、関数の中の break は外側のループを抜けられない
        assert_eq!(
            check("f[] (\n    break\n)"),
            Err("`break` は while の中でのみ使えます".into())
        );
        assert_eq!(
            check("while true (\n    if true (\n        break\n    )\n)"),
            Ok(())
        );
    }

    #[test]
    fn return_outside_function() {
        assert_eq!(check("# 1"), Err("`#` は関数の中でのみ使えます".into()));
    }

    #[test]
    fn duplicate_param() {
        assert_eq!(
            check("f[$a, $a] (\n)"),
            Err("引数 `$a` が重複しています".into())
        );
    }
}
