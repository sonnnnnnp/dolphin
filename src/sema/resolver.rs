//! 実行前の検査（design.md 5 章）
//!
//! - 関数はトップレベルにだけ定義でき、名前は重複せず、組み込み関数とも重ならない
//! - 呼び出す関数が定義されていて、引数の数が合っている（組み込み関数は個数を実行時に検査）
//! - `#` は関数の中でだけ使える
//! - `break` と `continue` は `while` の中でだけ使える
//! - 変数は、読む前に宣言されている（`@x := ...`）。代入できるのも宣言済みの変数だけ。
//!   同じスコープでの二重宣言は禁止（内側のスコープでの宣言は外側を隠すだけで、許す）
//!
//! スコープは `( )` のブロックと関数ごとに 1 つ。トップレベルの文は書かれた順に検査する。
//! 関数の中からは、そのファイルのトップレベルで宣言される変数すべてが見える
//! （関数は定義より前の行から呼べるので、宣言の位置は問わない。実行時に未宣言なら実行時エラー）

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
/// プログラム内で同じ名前を定義した場合はそちらが優先（REPL での再定義）。
/// `has_global` は、前の入力（REPL）ですでに宣言されているトップレベルの変数か
pub fn resolve(
    program: &Program,
    lookup: impl Fn(&str) -> Option<Callee>,
    has_global: impl Fn(&str) -> bool,
) -> RResult {
    let mut functions = HashMap::new();
    let mut hoisted = HashSet::new();
    for stmt in &program.stmts {
        match &stmt.kind {
            StmtKind::FuncDef(def) => {
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
            StmtKind::Declare { target, .. } => {
                hoisted.insert(target.0.as_str());
            }
            _ => {}
        }
    }

    let resolver = Resolver {
        functions,
        hoisted,
        lookup: &lookup,
        has_global: &has_global,
    };
    let mut top = Ctx {
        scopes: vec![HashSet::new()],
        in_func: false,
    };
    for stmt in &program.stmts {
        match &stmt.kind {
            StmtKind::FuncDef(def) => resolver.func_def(def)?,
            _ => resolver.stmt(stmt, &mut top, false)?,
        }
    }
    Ok(())
}

/// いま検査している場所のスコープ
struct Ctx<'a> {
    /// 外側から順。各スコープで宣言済みの変数名
    scopes: Vec<HashSet<&'a str>>,
    in_func: bool,
}

struct Resolver<'a> {
    /// 関数名 → 引数の数
    functions: HashMap<&'a str, usize>,
    /// トップレベルで宣言される変数すべて（関数の中から見える）
    hoisted: HashSet<&'a str>,
    lookup: &'a dyn Fn(&str) -> Option<Callee>,
    has_global: &'a dyn Fn(&str) -> bool,
}

impl<'a> Resolver<'a> {
    fn func_def(&self, def: &'a FuncDef) -> RResult {
        let mut params = HashSet::new();
        for param in &def.params {
            if !params.insert(param.as_str()) {
                return Err(Diagnostic::new(
                    def.span,
                    format!("引数 `@{param}` が重複しています"),
                ));
            }
        }
        // 本体は引数と同じスコープ（引数と同じ名前を `:=` で宣言し直すとエラー）
        let mut ctx = Ctx {
            scopes: vec![params],
            in_func: true,
        };
        self.stmts(&def.body, &mut ctx, false)
    }

    fn stmts(&self, block: &'a [Stmt], ctx: &mut Ctx<'a>, in_loop: bool) -> RResult {
        block
            .iter()
            .try_for_each(|stmt| self.stmt(stmt, ctx, in_loop))
    }

    /// 新しいスコープに入ってブロックを検査する。`in_loop` は `while` の中か
    /// （`break` / `continue` を使えるか）
    fn block(&self, block: &'a [Stmt], ctx: &mut Ctx<'a>, in_loop: bool) -> RResult {
        ctx.scopes.push(HashSet::new());
        let result = self.stmts(block, ctx, in_loop);
        ctx.scopes.pop();
        result
    }

    fn stmt(&self, stmt: &'a Stmt, ctx: &mut Ctx<'a>, in_loop: bool) -> RResult {
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
                self.expr(cond, ctx)?;
                self.block(then_block, ctx, in_loop)?;
                match else_block {
                    Some(block) => self.block(block, ctx, in_loop),
                    None => Ok(()),
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, ctx)?;
                self.block(body, ctx, true)
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
                if !ctx.in_func {
                    return Err(Diagnostic::new(stmt.span, "`#` は関数の中でのみ使えます"));
                }
                match value {
                    Some(value) => self.expr(value, ctx),
                    None => Ok(()),
                }
            }
            StmtKind::Declare { target, value } => {
                // `@x := @x + 1` の右辺の `@x` は、宣言する前の `@x`（外側）を指す
                self.expr(value, ctx)?;
                let scope = ctx.scopes.last_mut().expect("スコープは最低 1 つある");
                if !scope.insert(target.0.as_str()) {
                    return Err(Diagnostic::new(
                        stmt.span,
                        format!("変数 `{target}` はすでに宣言されています"),
                    ));
                }
                Ok(())
            }
            StmtKind::Assign {
                target,
                indices,
                value,
            } => {
                if !self.visible(target, ctx) {
                    let message = if indices.is_empty() {
                        format!(
                            "宣言されていない変数 `{target}` に代入しています（`{target} := ...` で宣言してください）"
                        )
                    } else {
                        format!("未定義の変数 `{target}`")
                    };
                    return Err(Diagnostic::new(stmt.span, message));
                }
                for index in indices {
                    self.expr(index, ctx)?;
                }
                self.expr(value, ctx)
            }
            StmtKind::Expr(expr) => self.expr(expr, ctx),
        }
    }

    fn expr(&self, expr: &'a Expr, ctx: &Ctx<'a>) -> RResult {
        match &expr.kind {
            ExprKind::Number(_) | ExprKind::Bool(_) => Ok(()),
            ExprKind::Str(parts) => parts.iter().try_for_each(|part| match part {
                StrPart::Var(var) => self.var(var, expr.span, ctx),
                StrPart::Text(_) => Ok(()),
            }),
            ExprKind::Var(var) => self.var(var, expr.span, ctx),
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
                args.iter().try_for_each(|arg| self.expr(arg, ctx))
            }
            ExprKind::Array(items) => items.iter().try_for_each(|item| self.expr(item, ctx)),
            ExprKind::Map(entries) => entries.iter().try_for_each(|(key, value)| {
                self.expr(key, ctx)?;
                self.expr(value, ctx)
            }),
            // モジュールの中身は実行時にしか分からない
            ExprKind::Member { target, .. } => self.expr(target, ctx),
            ExprKind::MemberCall { target, args, .. } => {
                self.expr(target, ctx)?;
                args.iter().try_for_each(|arg| self.expr(arg, ctx))
            }
            ExprKind::Index { target, index } => {
                self.expr(target, ctx)?;
                self.expr(index, ctx)
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, ctx),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, ctx)?;
                self.expr(rhs, ctx)
            }
        }
    }

    /// 変数を読めるか
    fn var(&self, var: &Var, span: Span, ctx: &Ctx<'a>) -> RResult {
        if self.visible(var, ctx) {
            Ok(())
        } else {
            Err(Diagnostic::new(span, format!("未定義の変数 `{var}`")))
        }
    }

    /// この場所から `var` が見えるか。内側のスコープから順に探し、
    /// 関数の中ならトップレベルで宣言される変数も見える
    fn visible(&self, var: &Var, ctx: &Ctx<'a>) -> bool {
        let name = var.0.as_str();
        ctx.scopes.iter().any(|scope| scope.contains(name))
            || (ctx.in_func && self.hoisted.contains(name))
            || (self.has_global)(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{lexer::tokenize, parser::parse};

    fn check(src: &str) -> Result<(), String> {
        let program = parse(&tokenize(src).unwrap()).unwrap();
        resolve(
            &program,
            |name| (name == "log").then_some(Callee::Native),
            |name| name == "prev",
        )
        .map_err(|e| e.message)
    }

    #[test]
    fn valid_program() {
        let src = "\
log[add[1, 2]]
add[@a, @b] (
    @sum := 0
    if @a > 0 (
        @sum = @a + @b
    )
    # @sum
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
            check("f[@a] (\n)\nf[1, 2]"),
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
            check("log[@x] (\n)"),
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
    fn undeclared_variable() {
        assert_eq!(check("log[@x]"), Err("未定義の変数 `@x`".into()));
        assert_eq!(
            check("f[@a] (\n    # @b\n)"),
            Err("未定義の変数 `@b`".into())
        );
        // 文字列の中の変数も検査する
        assert_eq!(check("log[\"@x\"]"), Err("未定義の変数 `@x`".into()));
        assert_eq!(
            check("f[@a] (\n    log[\"@a @b\"]\n)"),
            Err("未定義の変数 `@b`".into())
        );
    }

    #[test]
    fn assign_needs_declaration() {
        assert_eq!(
            check("@x = 1"),
            Err(
                "宣言されていない変数 `@x` に代入しています（`@x := ...` で宣言してください）"
                    .into()
            )
        );
        assert_eq!(check("@x := 1\n@x = 2"), Ok(()));
        // 添字つきの代入は、変数を読む
        assert_eq!(check("@a{0} = 1"), Err("未定義の変数 `@a`".into()));
    }

    #[test]
    fn declare_before_use_at_top_level() {
        // トップレベルの文は書かれた順に検査する
        assert_eq!(check("log[@x]\n@x := 1"), Err("未定義の変数 `@x`".into()));
        // 自分自身の右辺は、宣言する前の変数を指す
        assert_eq!(check("@x := @x + 1"), Err("未定義の変数 `@x`".into()));
    }

    #[test]
    fn functions_see_top_level_variables() {
        // 関数はどこに書いても、トップレベルの変数が見える
        assert_eq!(check("inc[] (\n    @n = @n + 1\n)\n@n := 0\ninc[]"), Ok(()));
        // ブロックの中で宣言した変数は、関数からは見えない
        assert_eq!(
            check("if true (\n    @n := 0\n)\nf[] (\n    # @n\n)"),
            Err("未定義の変数 `@n`".into())
        );
    }

    #[test]
    fn variables_from_previous_input() {
        assert_eq!(check("log[@prev]\n@prev = 1"), Ok(()));
    }

    #[test]
    fn block_scope() {
        // ブロックの中で宣言した変数は、ブロックの外では使えない
        assert_eq!(
            check("if true (\n    @x := 1\n)\nlog[@x]"),
            Err("未定義の変数 `@x`".into())
        );
        // 外側の変数は内側から読み書きできる
        assert_eq!(
            check("@x := 1\nwhile true (\n    @x = @x + 1\n    break\n)"),
            Ok(())
        );
    }

    #[test]
    fn redeclare() {
        assert_eq!(
            check("@x := 1\n@x := 2"),
            Err("変数 `@x` はすでに宣言されています".into())
        );
        // 内側のスコープでは、外側の変数を隠せる
        assert_eq!(check("@x := 1\nif true (\n    @x := 2\n)"), Ok(()));
        // 引数と同じスコープなので、宣言し直せない
        assert_eq!(
            check("f[@a] (\n    @a := 1\n)"),
            Err("変数 `@a` はすでに宣言されています".into())
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
            check("f[@a, @a] (\n)"),
            Err("引数 `@a` が重複しています".into())
        );
    }
}
