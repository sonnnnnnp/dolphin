//! 1 つのソースを解析して、名前の使われ方（どこで宣言されて、どこで使われているか）を調べる。
//! 規則は Resolver と同じ（design.md 5.33）。構文木は文・式の開始位置しか持たないので、
//! 宣言の `@名前` の位置は、その文の位置から原文を探して決める。

use std::collections::HashMap;
use std::rc::Rc;

use crate::diag::Diagnostic;
use crate::syntax::ast::{Expr, ExprKind, FuncDef, Pattern, Program, Stmt, StmtKind, StrSegment};
use crate::syntax::span::Span;
use crate::syntax::{lexer, parser};

/// 宣言の場所と中身
#[derive(Debug)]
pub struct Def {
    pub name: String,
    /// 名前の先頭（変数なら `@` の位置）
    pub span: Span,
    /// 名前の長さ（文字数。変数なら `@` を含む）
    pub len: usize,
    pub kind: DefKind,
    /// ホバーに出す説明（形や宣言の行と、直前のコメント）
    pub hover: String,
    /// `@m := import["相対パス"]` のとき、そのパス
    pub import: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DefKind {
    Func,
    Var,
}

#[derive(Debug, Clone)]
pub enum Target {
    Def(Rc<Def>),
    /// `@m.name` / `@m.name[…]`。`path` は `@m` が import しているファイル
    Member {
        path: String,
        name: String,
    },
}

#[derive(Debug)]
pub struct Ref {
    pub span: Span,
    pub len: usize,
    pub target: Target,
}

/// アウトライン（トップレベルの関数と変数）
#[derive(Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: DefKind,
    pub span: Span,
    pub len: usize,
    /// 範囲の最初と最後の行（1 始まり）
    pub first_line: usize,
    pub last_line: usize,
}

pub struct Analysis {
    pub refs: Vec<Ref>,
    /// ファイルの中のすべての宣言（出てきた順）
    pub defs: Vec<Rc<Def>>,
    pub symbols: Vec<Symbol>,
    /// トップレベルの関数と変数（import されたときの定義先）
    pub exports: HashMap<String, Rc<Def>>,
}

/// 構文エラーのあるソースは解析しない
pub fn analyze(src: &str) -> Result<Analysis, Diagnostic> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    Ok(Walker::new(src).run(&program))
}

struct Walker<'a> {
    lines: Vec<&'a str>,
    functions: HashMap<String, Rc<Def>>,
    hoisted: HashMap<String, Rc<Def>>,
    /// 内側のスコープが後ろ
    scopes: Vec<HashMap<String, Rc<Def>>>,
    refs: Vec<Ref>,
    defs: Vec<Rc<Def>>,
}

impl<'a> Walker<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            lines: src.lines().collect(),
            functions: HashMap::new(),
            hoisted: HashMap::new(),
            scopes: Vec::new(),
            refs: Vec::new(),
            defs: Vec::new(),
        }
    }

    fn run(mut self, program: &Program) -> Analysis {
        // 関数は定義より前から呼べる。トップレベルの変数も、関数の中からは宣言の位置を問わず見える
        for stmt in &program.stmts {
            match &stmt.kind {
                StmtKind::FuncDef(def) => {
                    let d = self.func_def(def);
                    self.defs.push(d.clone());
                    self.refs.push(Ref {
                        span: d.span,
                        len: d.len,
                        target: Target::Def(d.clone()),
                    });
                    self.functions.entry(def.name.clone()).or_insert(d);
                }
                StmtKind::Declare { target, value } => {
                    let d = self.var_def(&target.0, stmt.span, import_path(value));
                    self.hoisted.entry(target.0.clone()).or_insert(d);
                }
                StmtKind::Destructure {
                    pattern,
                    declare: true,
                    ..
                } => {
                    for var in pattern.vars() {
                        let d = self.var_def(&var.0, stmt.span, None);
                        self.hoisted.entry(var.0.clone()).or_insert(d);
                    }
                }
                _ => {}
            }
        }

        self.scopes.push(HashMap::new());
        for stmt in &program.stmts {
            match &stmt.kind {
                StmtKind::FuncDef(def) => {
                    let saved = std::mem::take(&mut self.scopes);
                    self.func_body(def);
                    self.scopes = saved;
                }
                _ => self.stmt(stmt),
            }
        }

        let mut exports = self.hoisted.clone();
        exports.extend(self.functions.clone());
        let symbols = self.symbols(program);
        Analysis {
            refs: self.refs,
            defs: self.defs,
            symbols,
            exports,
        }
    }

    // ---- 宣言の位置と説明 ----

    fn line(&self, line: usize) -> &'a str {
        self.lines.get(line - 1).copied().unwrap_or("")
    }

    /// `from` 以降で最初に現れる `@name`（直後が名前の続きでないもの）の位置
    fn find_var(&self, name: &str, from: Span) -> Span {
        let needle: Vec<char> = format!("@{name}").chars().collect();
        for line in from.line..=self.lines.len() {
            let chars: Vec<char> = self.line(line).chars().collect();
            let start = if line == from.line { from.col - 1 } else { 0 };
            for i in start..chars.len() {
                if chars[i..].starts_with(&needle)
                    && !chars
                        .get(i + needle.len())
                        .is_some_and(|c| is_ident_char(*c))
                {
                    return Span { line, col: i + 1 };
                }
            }
        }
        from
    }

    /// 直前の連続する `//` コメント
    fn doc_above(&self, line: usize) -> String {
        let mut docs = Vec::new();
        let mut n = line;
        while n > 1 {
            let text = self.line(n - 1).trim();
            let Some(comment) = text.strip_prefix("//") else {
                break;
            };
            docs.push(comment.trim().to_string());
            n -= 1;
        }
        docs.reverse();
        docs.join("\n")
    }

    fn hover_text(&self, code: &str, line: usize) -> String {
        let doc = self.doc_above(line);
        if doc.is_empty() {
            code.to_string()
        } else {
            format!("{code}\n\n{doc}")
        }
    }

    fn var_def(&self, name: &str, from: Span, import: Option<String>) -> Rc<Def> {
        let span = self.find_var(name, from);
        let code = self.line(span.line).trim().to_string();
        Rc::new(Def {
            name: name.to_string(),
            span,
            len: name.chars().count() + 1,
            kind: DefKind::Var,
            hover: self.hover_text(&code, span.line),
            import,
        })
    }

    fn func_def(&self, def: &FuncDef) -> Rc<Def> {
        // 形は原文の `name[ … ]` をそのまま使う
        let chars: Vec<char> = self.line(def.span.line).chars().collect();
        let from = def.span.col - 1;
        let mut depth = 0;
        let mut end = chars.len();
        for (i, c) in chars.iter().enumerate().skip(from) {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let sig: String = chars[from.min(end)..end].iter().collect();
        Rc::new(Def {
            name: def.name.clone(),
            span: def.span,
            len: def.name.chars().count(),
            kind: DefKind::Func,
            hover: self.hover_text(&sig, def.span.line),
            import: None,
        })
    }

    // ---- 走査 ----

    fn lookup(&self, name: &str) -> Option<Rc<Def>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .or_else(|| self.hoisted.get(name))
            .cloned()
    }

    fn declare(&mut self, def: Rc<Def>) {
        self.defs.push(def.clone());
        self.refs.push(Ref {
            span: def.span,
            len: def.len,
            target: Target::Def(def.clone()),
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(def.name.clone(), def);
        }
    }

    fn use_var(&mut self, name: &str, span: Span) {
        if let Some(def) = self.lookup(name) {
            self.refs.push(Ref {
                span,
                len: name.chars().count() + 1,
                target: Target::Def(def),
            });
        }
    }

    fn use_func(&mut self, name: &str, span: Span) {
        if let Some(def) = self.functions.get(name) {
            self.refs.push(Ref {
                span,
                len: name.chars().count(),
                target: Target::Def(def.clone()),
            });
        }
    }

    /// 関数の本体（引数と同じスコープ）。`scopes` は関数を囲むスコープ
    fn func_body(&mut self, def: &FuncDef) {
        self.scopes.push(HashMap::new());
        for param in &def.params {
            if let Some(default) = &param.default {
                self.expr(default);
            }
            let d = self.var_def(&param.name, def.span, None);
            self.declare(d);
        }
        if let Some(rest) = &def.rest {
            let d = self.var_def(rest, def.span, None);
            self.declare(d);
        }
        self.stmts(&def.body);
        self.scopes.pop();
    }

    fn block(&mut self, body: &[Stmt]) {
        self.scopes.push(HashMap::new());
        self.stmts(body);
        self.scopes.pop();
    }

    fn stmts(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::FuncDef(_) | StmtKind::Break | StmtKind::Continue => {}
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::For { var, iter, body } => {
                self.expr(iter);
                self.scopes.push(HashMap::new());
                let d = self.var_def(&var.0, stmt.span, None);
                self.declare(d);
                self.stmts(body);
                self.scopes.pop();
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Declare { target, value } => {
                self.expr(value);
                let d = self.var_def(&target.0, stmt.span, import_path(value));
                self.declare(d);
            }
            StmtKind::Assign {
                target,
                indices,
                value,
            } => {
                let at = self.find_var(&target.0, stmt.span);
                self.use_var(&target.0, at);
                for index in indices {
                    self.expr(index);
                }
                self.expr(value);
            }
            StmtKind::Destructure {
                pattern,
                value,
                declare,
            } => {
                self.expr(value);
                self.pattern(pattern, stmt.span, *declare);
            }
            StmtKind::Expr(expr) => self.expr(expr),
        }
    }

    fn pattern(&mut self, pattern: &Pattern, from: Span, declare: bool) {
        for var in pattern.vars() {
            if declare {
                let d = self.var_def(&var.0, from, None);
                self.declare(d);
            } else {
                let at = self.find_var(&var.0, from);
                self.use_var(&var.0, at);
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Number(_) | ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Nil => {}
            ExprKind::Str(segments) => {
                // 文字列の中の `@name` は、式の位置が文字列の先頭になっているので、原文から探す。
                // `@{ 式 }` の式は、字句の位置を持っている
                let mut from = expr.span;
                for segment in segments {
                    match segment {
                        StrSegment::Expr(
                            e @ Expr {
                                kind: ExprKind::Var(var),
                                ..
                            },
                        ) if e.span == expr.span => {
                            let at = self.find_var(&var.0, from);
                            self.use_var(&var.0, at);
                            from = Span {
                                line: at.line,
                                col: at.col + var.0.chars().count() + 1,
                            };
                        }
                        StrSegment::Expr(e) => {
                            self.expr(e);
                            // 次の `@name` を探す位置を、この `@{ }` の後ろまで進める
                            from = self.after_interpolation(e.span);
                        }
                        StrSegment::Text(_) => {}
                    }
                }
            }
            ExprKind::Var(var) => self.use_var(&var.0, expr.span),
            ExprKind::Call { name, args } => {
                self.use_func(name, expr.span);
                self.exprs(args);
            }
            ExprKind::FuncRef(name) => self.use_func(name, expr.span),
            ExprKind::Lambda(def) => self.func_body(def),
            ExprKind::Spread(inner) | ExprKind::Try(inner) => self.expr(inner),
            ExprKind::If {
                cond,
                then_expr,
                else_expr,
            } => {
                self.expr(cond);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            ExprKind::CallValue { callee, args } => {
                self.expr(callee);
                self.exprs(args);
            }
            ExprKind::Array(items) => self.exprs(items),
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Member { target, name } => self.member(target, name, &[]),
            ExprKind::MemberCall { target, name, args } => self.member(target, name, args),
            ExprKind::Index { target, index } => {
                self.expr(target);
                self.expr(index);
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
        }
    }

    /// `at`（`@{ }` の中の位置）を含む `@{ }` の、閉じ `}` の次の位置
    fn after_interpolation(&self, at: Span) -> Span {
        let chars: Vec<char> = self.line(at.line).chars().collect();
        match lexer::interpolation_end(&chars, at.col - 1) {
            Some(end) => Span {
                line: at.line,
                col: end + 2,
            },
            None => at,
        }
    }

    fn exprs(&mut self, exprs: &[Expr]) {
        for e in exprs {
            self.expr(e);
        }
    }

    /// `@m.name`。`@m` が相対パスの import なら、その先の定義を指す
    fn member(&mut self, target: &Expr, name: &str, args: &[Expr]) {
        self.expr(target);
        if let ExprKind::Var(var) = &target.kind {
            let path = self.lookup(&var.0).and_then(|d| d.import.clone());
            if let Some(path) = path.filter(|p| !p.starts_with("std:")) {
                // `@m.name` の名前は、`@m.` のすぐ後ろ
                let col = target.span.col + var.0.chars().count() + 2;
                let span = Span {
                    line: target.span.line,
                    col,
                };
                self.refs.push(Ref {
                    span,
                    len: name.chars().count(),
                    target: Target::Member {
                        path,
                        name: name.to_string(),
                    },
                });
            }
        }
        self.exprs(args);
    }

    // ---- アウトライン ----

    fn symbols(&self, program: &Program) -> Vec<Symbol> {
        let last_nonblank = (1..=self.lines.len())
            .rev()
            .find(|&n| !self.line(n).trim().is_empty())
            .unwrap_or(1);
        let mut symbols = Vec::new();
        for (i, stmt) in program.stmts.iter().enumerate() {
            let (name, kind, def) = match &stmt.kind {
                StmtKind::FuncDef(def) => (
                    def.name.clone(),
                    DefKind::Func,
                    self.functions.get(&def.name),
                ),
                StmtKind::Declare { target, .. } => {
                    (target.0.clone(), DefKind::Var, self.hoisted.get(&target.0))
                }
                _ => continue,
            };
            let Some(def) = def else { continue };
            // 次の文の手前まで（間の空行と、次の文の直前のコメントは含めない）
            let mut last_line = match program.stmts.get(i + 1) {
                Some(next) => next.span.line.saturating_sub(1),
                None => last_nonblank,
            };
            while last_line > stmt.span.line
                && (self.line(last_line).trim().is_empty()
                    || self.line(last_line).trim().starts_with("//"))
            {
                last_line -= 1;
            }
            symbols.push(Symbol {
                name,
                kind,
                span: def.span,
                len: def.len,
                first_line: stmt.span.line,
                last_line: last_line.max(stmt.span.line),
            });
        }
        symbols
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `import["a.dol"]` の `a.dol`（式展開のない文字列だけ）
fn import_path(value: &Expr) -> Option<String> {
    let ExprKind::Call { name, args } = &value.kind else {
        return None;
    };
    if name != "import" {
        return None;
    }
    let [
        Expr {
            kind: ExprKind::Str(segments),
            ..
        },
    ] = args.as_slice()
    else {
        return None;
    };
    match segments.as_slice() {
        [StrSegment::Text(text)] => Some(text.clone()),
        _ => None,
    }
}

impl Analysis {
    /// `def` を指す場所（宣言そのものも含む）
    pub fn refs_to<'a>(&'a self, def: &'a Rc<Def>) -> impl Iterator<Item = &'a Ref> {
        self.refs
            .iter()
            .filter(move |r| matches!(&r.target, Target::Def(d) if Rc::ptr_eq(d, def)))
    }

    /// 位置（1 始まりの行・文字）にある名前の使われ方
    pub fn ref_at(&self, line: usize, col: usize) -> Option<&Ref> {
        self.refs
            .iter()
            .find(|r| r.span.line == line && r.span.col <= col && col < r.span.col + r.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_span(a: &Analysis, line: usize, col: usize) -> Option<Span> {
        match &a.ref_at(line, col)?.target {
            Target::Def(d) => Some(d.span),
            Target::Member { .. } => None,
        }
    }

    const SRC: &str = "\
// 足し算
add[@a, @b = 1] (
    # @a + @b
)

@total := add[1]
if @total > 0 (
    @total := 5
    log[\"sum @total\"]
)
@f := [@x] ( # @x + @total )
";

    #[test]
    fn finds_definitions() {
        let a = analyze(SRC).unwrap();
        // `@a` の使用 → 引数 `@a`
        assert_eq!(target_span(&a, 3, 7), Some(Span { line: 2, col: 5 }));
        // `add[1]` → 関数定義
        assert_eq!(target_span(&a, 6, 11), Some(Span { line: 2, col: 1 }));
        // if の中の `@total` は内側の宣言を指す
        assert_eq!(target_span(&a, 7, 5), Some(Span { line: 6, col: 1 }));
        // 文字列の中の `@total` も内側
        assert_eq!(target_span(&a, 9, 15), Some(Span { line: 8, col: 5 }));
        // 無名関数は外側の `@total` を捕まえる
        assert_eq!(target_span(&a, 11, 22), Some(Span { line: 6, col: 1 }));
    }

    #[test]
    fn interpolation_expressions_are_walked() {
        let a = analyze(
            "@x := 1
log[\"a @{@x + 1} b @x\"]
",
        )
        .unwrap();
        let spans: Vec<_> = a.refs.iter().map(|r| (r.span.line, r.span.col)).collect();
        assert_eq!(
            spans.iter().filter(|s| **s == (2, 10)).count(),
            1,
            "{spans:?}"
        );
        assert!(spans.contains(&(2, 20)), "{spans:?}");
    }

    #[test]
    fn hover_has_signature_and_doc() {
        let a = analyze(SRC).unwrap();
        let Target::Def(d) = &a.ref_at(6, 11).unwrap().target else {
            panic!()
        };
        assert_eq!(d.hover, "add[@a, @b = 1]\n\n足し算");
        let Target::Def(v) = &a.ref_at(7, 5).unwrap().target else {
            panic!()
        };
        assert_eq!(v.hover, "@total := add[1]");
    }

    #[test]
    fn outline_ranges() {
        let a = analyze(SRC).unwrap();
        let names: Vec<_> = a
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.first_line, s.last_line))
            .collect();
        assert_eq!(names, [("add", 2, 4), ("total", 6, 6), ("f", 11, 11)]);
    }

    #[test]
    fn import_member_points_to_file() {
        let a = analyze("@m := import[\"lib.dol\"]\nlog[@m.go[1]]\n").unwrap();
        let r = a.ref_at(2, 9).unwrap();
        let Target::Member { path, name } = &r.target else {
            panic!("{r:?}")
        };
        assert_eq!((path.as_str(), name.as_str()), ("lib.dol", "go"));
    }

    #[test]
    fn syntax_error_is_reported() {
        assert!(analyze("@x := (").is_err());
    }
}
