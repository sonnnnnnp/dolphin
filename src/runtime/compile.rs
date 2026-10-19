//! AST をバイトコードにコンパイルする（design.md 5.45）
//!
//! 変数の置き場所は評価器と同じ（名前で引く `Env`）。ここでは制御の流れだけを命令にする。
//! 位置情報は、評価器が使っていた式・文の位置をそのまま命令に付ける

use std::collections::HashMap;

use super::bytecode::{Chunk, Op};
use super::value::Value;
use crate::syntax::ast::{BinaryOp, Expr, ExprKind, FuncDef, Stmt, StmtKind, StrSegment};
use crate::syntax::span::Span;

/// ファイルの本体をコンパイルする。`keep_last` で最後の文が式なら、その値が戻り値になる
pub fn compile_program(stmts: &[Stmt], keep_last: bool) -> Chunk {
    let mut c = Compiler::new();
    let end = stmts.last().map_or(Span { line: 1, col: 1 }, |s| s.span);
    match stmts.split_last() {
        Some((last, rest)) if keep_last => {
            c.block(rest);
            match &last.kind {
                StmtKind::Expr(expr) => c.expr(expr),
                _ => {
                    c.stmt(last);
                    c.nil(end);
                }
            }
        }
        _ => {
            c.block(stmts);
            c.nil(end);
        }
    }
    c.emit(Op::Return, end);
    c.chunk
}

/// 関数の本体をコンパイルする。引数の宣言は呼び出し側（VM）が行い、ここは既定値の評価から始まる
pub fn compile_function(def: &FuncDef) -> Chunk {
    let mut c = Compiler::new();
    for (i, param) in def.params.iter().enumerate() {
        if let Some(default) = &param.default {
            let skip = c.emit(
                Op::SkipIfGiven {
                    index: i as u32,
                    target: 0,
                },
                default.span,
            );
            c.expr(default);
            let name = c.name(&param.name);
            c.emit(Op::Declare(name), default.span);
            c.patch_here(skip);
        }
    }
    c.block(&def.body);
    c.nil(def.span);
    c.emit(Op::Return, def.span);
    c.chunk
}

struct LoopCtx {
    /// ループに入ったときのスコープの深さ
    depth: u32,
    /// `break` の `Jump`（ループの外に飛ぶ）
    breaks: Vec<usize>,
    /// `continue` の `Jump`（次の繰り返しへ飛ぶ）
    continues: Vec<usize>,
}

struct Compiler {
    chunk: Chunk,
    names: HashMap<String, u32>,
    scope_depth: u32,
    loops: Vec<LoopCtx>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            chunk: Chunk {
                code: Vec::new(),
                spans: Vec::new(),
                consts: Vec::new(),
                names: Vec::new(),
                defs: Vec::new(),
                patterns: Vec::new(),
            },
            names: HashMap::new(),
            scope_depth: 0,
            loops: Vec::new(),
        }
    }

    fn emit(&mut self, op: Op, span: Span) -> usize {
        self.chunk.code.push(op);
        self.chunk.spans.push(span);
        self.chunk.code.len() - 1
    }

    fn here(&self) -> u32 {
        self.chunk.code.len() as u32
    }

    /// `at` の命令の行き先を、いまの位置にする
    fn patch_here(&mut self, at: usize) {
        let target = self.here();
        self.patch(at, target);
    }

    fn patch(&mut self, at: usize, to: u32) {
        match &mut self.chunk.code[at] {
            Op::Jump(t)
            | Op::JumpIfFalseCond(t)
            | Op::ShortCircuit(_, t)
            | Op::SkipIfGiven { target: t, .. }
            | Op::ForNext { end: t, .. } => *t = to,
            other => unreachable!("行き先のない命令を書き換えようとした: {other:?}"),
        }
    }

    fn name(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.names.get(name) {
            return i;
        }
        let i = self.chunk.names.len() as u32;
        self.chunk.names.push(name.to_string());
        self.names.insert(name.to_string(), i);
        i
    }

    fn constant(&mut self, value: Value, span: Span) {
        self.chunk.consts.push(value);
        let i = (self.chunk.consts.len() - 1) as u32;
        self.emit(Op::Const(i), span);
    }

    fn nil(&mut self, span: Span) {
        self.constant(Value::Nil, span);
    }

    // ---- 文 ----

    fn block(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    /// 新しいスコープに入ってブロックを実行する
    fn scoped_block(&mut self, stmts: &[Stmt], span: Span) {
        self.emit(Op::PushScope, span);
        self.scope_depth += 1;
        self.block(stmts);
        self.scope_depth -= 1;
        self.emit(Op::PopScope, span);
    }

    fn stmt(&mut self, stmt: &Stmt) {
        let span = stmt.span;
        match &stmt.kind {
            // 実行の前に登録済み
            StmtKind::FuncDef(_) => {}
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                self.expr(cond);
                let to_else = self.emit(Op::JumpIfFalseCond(0), cond.span);
                self.scoped_block(then_block, span);
                if let Some(else_block) = else_block {
                    let to_end = self.emit(Op::Jump(0), span);
                    self.patch_here(to_else);
                    self.scoped_block(else_block, span);
                    self.patch_here(to_end);
                } else {
                    self.patch_here(to_else);
                }
            }
            StmtKind::While { cond, body } => {
                let start = self.here();
                self.expr(cond);
                let to_end = self.emit(Op::JumpIfFalseCond(0), cond.span);
                self.emit(Op::LoopTick, span);
                self.loops.push(LoopCtx {
                    depth: self.scope_depth,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                self.scoped_block(body, span);
                self.emit(Op::Jump(start), span);
                let end = self.here();
                self.patch(to_end, end);
                let ctx = self.loops.pop().expect("ループに入っている");
                for at in ctx.breaks {
                    self.patch(at, end);
                }
                for at in ctx.continues {
                    self.patch(at, start);
                }
            }
            StmtKind::For { var, iter, body } => {
                self.expr(iter);
                self.emit(Op::ForPrep, iter.span);
                let next = self.here();
                let var = self.name(&var.0);
                let for_next = self.emit(Op::ForNext { end: 0, var }, span);
                self.loops.push(LoopCtx {
                    depth: self.scope_depth,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                // 繰り返しごとの新しいスコープ（ループ変数と本体）
                self.scope_depth += 1;
                self.block(body);
                self.scope_depth -= 1;
                self.emit(Op::PopScope, span);
                self.emit(Op::Jump(next), span);
                let exit = self.here();
                self.patch(for_next, exit);
                let ctx = self.loops.pop().expect("ループに入っている");
                for at in ctx.breaks {
                    self.patch(at, exit);
                }
                for at in ctx.continues {
                    self.patch(at, next);
                }
                // 準備で積んだ、要素の写しと位置
                self.emit(Op::Pop, span);
                self.emit(Op::Pop, span);
            }
            StmtKind::Return(value) => {
                match value {
                    Some(expr) => self.expr(expr),
                    None => self.nil(span),
                }
                self.emit(Op::Return, span);
            }
            StmtKind::Break => {
                let at = self.leave_loop_scopes(span);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.breaks.push(at);
                }
            }
            StmtKind::Continue => {
                let at = self.leave_loop_scopes(span);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.continues.push(at);
                }
            }
            StmtKind::Declare { target, value } => {
                self.expr(value);
                let name = self.name(&target.0);
                self.emit(Op::Declare(name), span);
            }
            StmtKind::Destructure {
                pattern,
                value,
                declare,
            } => {
                self.expr(value);
                self.chunk.patterns.push(pattern.clone());
                let pattern = (self.chunk.patterns.len() - 1) as u32;
                self.emit(
                    Op::Destructure {
                        pattern,
                        declare: *declare,
                    },
                    span,
                );
            }
            StmtKind::Assign {
                target,
                indices,
                value,
            } => {
                let name = self.name(&target.0);
                let Some((last, outer)) = indices.split_last() else {
                    self.expr(value);
                    self.emit(Op::AssignVar(name), span);
                    return;
                };
                // 配列とマップは参照なので、読んでいった先を書き換える。
                // `@m{"a"}{"b"} = v` は、外側の添字を読んでいって最後の 1 つだけ書く
                self.emit(Op::LoadVar(name), span);
                for index in outer {
                    self.emit(Op::RejectModule, span);
                    self.expr(index);
                    self.emit(
                        Op::IndexGet {
                            target: span,
                            key: index.span,
                        },
                        span,
                    );
                }
                self.emit(Op::RejectModule, span);
                self.expr(last);
                self.expr(value);
                self.emit(
                    Op::IndexSet {
                        target: span,
                        key: last.span,
                    },
                    span,
                );
            }
            StmtKind::Expr(expr) => {
                self.expr(expr);
                self.emit(Op::PopCheckError, expr.span);
            }
        }
    }

    /// `break` / `continue`: ループの中で開いたスコープを閉じて、行き先の決まっていない `Jump` を置く
    fn leave_loop_scopes(&mut self, span: Span) -> usize {
        let depth = self.loops.last().map_or(self.scope_depth, |ctx| ctx.depth);
        for _ in depth..self.scope_depth {
            self.emit(Op::PopScope, span);
        }
        self.emit(Op::Jump(0), span)
    }

    // ---- 式 ----

    fn expr(&mut self, expr: &Expr) {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Number(n) => self.constant(Value::Num(*n), span),
            ExprKind::Int(n) => self.constant(Value::Int(*n), span),
            ExprKind::Bool(b) => self.constant(Value::Bool(*b), span),
            ExprKind::Nil => self.nil(span),
            ExprKind::Str(parts) => {
                if let [StrSegment::Text(text)] = parts.as_slice() {
                    self.constant(Value::Str(text.as_str().into()), span);
                    return;
                }
                for part in parts {
                    match part {
                        StrSegment::Text(text) => {
                            self.constant(Value::Str(text.as_str().into()), span);
                        }
                        StrSegment::Expr(inner) => {
                            self.expr(inner);
                            self.emit(Op::ToDisplay, inner.span);
                        }
                    }
                }
                self.emit(Op::Concat(parts.len() as u32), span);
            }
            ExprKind::Var(var) => {
                let name = self.name(&var.0);
                self.emit(Op::LoadVar(name), span);
            }
            ExprKind::Array(items) => {
                if has_spread(items) {
                    self.build_list(items);
                } else {
                    for item in items {
                        self.expr(item);
                    }
                    self.emit(Op::MakeArray(items.len() as u32), span);
                }
            }
            ExprKind::Map(entries) => {
                for (key, value) in entries {
                    self.expr(key);
                    self.emit(Op::CheckKey, key.span);
                    self.expr(value);
                }
                self.emit(Op::MakeMap(entries.len() as u32), span);
            }
            ExprKind::Member { target, name } => {
                self.expr(target);
                let name = self.name(name);
                self.emit(
                    Op::Member {
                        name,
                        target: target.span,
                    },
                    span,
                );
            }
            ExprKind::FuncRef(name) => {
                let name = self.name(name);
                self.emit(Op::FuncRef(name), span);
            }
            ExprKind::If {
                cond,
                then_expr,
                else_expr,
            } => {
                self.expr(cond);
                let to_else = self.emit(Op::JumpIfFalseCond(0), cond.span);
                self.expr(then_expr);
                let to_end = self.emit(Op::Jump(0), span);
                self.patch_here(to_else);
                self.expr(else_expr);
                self.patch_here(to_end);
            }
            // 引数と配列リテラルの外では、`...` は構文解析で弾かれる
            ExprKind::Spread(inner) => self.expr(inner),
            ExprKind::Try(inner) => {
                self.expr(inner);
                self.emit(Op::Try, span);
            }
            ExprKind::Lambda(def) => {
                self.chunk.defs.push(def.clone());
                let i = (self.chunk.defs.len() - 1) as u32;
                self.emit(Op::MakeClosure(i), span);
            }
            ExprKind::CallValue { callee, args } => {
                self.expr(callee);
                if has_spread(args) {
                    self.build_list(args);
                    self.emit(
                        Op::CallValueSpread {
                            callee: callee.span,
                        },
                        span,
                    );
                } else {
                    for arg in args {
                        self.expr(arg);
                    }
                    self.emit(
                        Op::CallValue {
                            argc: args.len() as u32,
                            callee: callee.span,
                        },
                        span,
                    );
                }
            }
            ExprKind::MemberCall { target, name, args } => {
                self.expr(target);
                let name = self.name(name);
                if has_spread(args) {
                    self.build_list(args);
                    self.emit(
                        Op::MemberCallSpread {
                            name,
                            target: target.span,
                        },
                        span,
                    );
                } else {
                    for arg in args {
                        self.expr(arg);
                    }
                    self.emit(
                        Op::MemberCall {
                            name,
                            argc: args.len() as u32,
                            target: target.span,
                        },
                        span,
                    );
                }
            }
            ExprKind::Index { target, index } => {
                self.expr(target);
                self.expr(index);
                self.emit(
                    Op::IndexGet {
                        target: target.span,
                        key: index.span,
                    },
                    span,
                );
            }
            ExprKind::Unary { op, operand } => {
                self.expr(operand);
                self.emit(Op::Unary(*op), span);
            }
            ExprKind::Binary { op, lhs, rhs } => {
                if let BinaryOp::And | BinaryOp::Or = op {
                    // 短絡評価。両辺とも真偽値に限る
                    self.expr(lhs);
                    self.emit(Op::LogicOperand(*op), lhs.span);
                    let short = self.emit(Op::ShortCircuit(*op, 0), span);
                    self.expr(rhs);
                    self.emit(Op::LogicOperand(*op), rhs.span);
                    self.patch_here(short);
                } else {
                    self.expr(lhs);
                    self.expr(rhs);
                    self.emit(Op::Binary(*op), span);
                }
            }
            ExprKind::Call { name, args } => {
                let name = self.name(name);
                if has_spread(args) {
                    self.build_list(args);
                    self.emit(Op::CallSpread { name }, span);
                } else {
                    for arg in args {
                        self.expr(arg);
                    }
                    self.emit(
                        Op::Call {
                            name,
                            argc: args.len() as u32,
                        },
                        span,
                    );
                }
            }
        }
    }

    /// 展開（`...式`）を含む並びを、1 つの配列にして積む
    fn build_list(&mut self, items: &[Expr]) {
        let first = items.first().map_or(Span { line: 1, col: 1 }, |e| e.span);
        self.emit(Op::ArrayNew, first);
        for item in items {
            if let ExprKind::Spread(inner) = &item.kind {
                self.expr(inner);
                self.emit(Op::ArrayExtend, inner.span);
            } else {
                self.expr(item);
                self.emit(Op::ArrayAppend, item.span);
            }
        }
    }
}

fn has_spread(items: &[Expr]) -> bool {
    items.iter().any(|e| matches!(e.kind, ExprKind::Spread(_)))
}
