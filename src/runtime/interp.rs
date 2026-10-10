//! AST の木たどり評価器

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::env::{Frame, Scope};
use super::error::{RuntimeError, TraceEntry};
use super::map::Map;
use super::module::Module;
use super::value::Value;
use crate::diag::Origin;
use crate::sema::resolver::{self, Callee};
use crate::syntax::ast::{
    BinaryOp, Expr, ExprKind, FuncDef, Program, Stmt, StmtKind, UnaryOp, Var,
};
use crate::syntax::lexer::StrPart;
use crate::syntax::span::Span;
use crate::syntax::{lexer, parser};

/// 組み込み関数。エラー位置は呼び出し側で付けるので、メッセージだけ返す
pub type NativeFn = fn(&mut Interpreter<'_>, &[Value]) -> Result<Value, String>;

/// 関数呼び出しの深さの上限。超えたら無限再帰とみなしてエラーにする
pub const MAX_DEPTH: usize = 1000;

type RResult<T> = Result<T, RuntimeError>;

/// 文を実行した後の制御の流れ
enum Flow {
    Normal,
    /// `#` が実行された。関数の終わりまで戻る
    Return(Value),
    /// `break` が実行された。いちばん内側の while を抜ける
    Break,
    /// `continue` が実行された。いちばん内側の while の条件に戻る
    Continue,
}

/// import したファイルの状態（design.md 5.7）
enum ModuleState {
    /// 実行中。この状態のファイルをもう一度 import したら循環
    Loading,
    Loaded(Rc<Module>),
}

pub struct Interpreter<'a> {
    /// `log` の出力先。テストでは Vec<u8> を渡して出力を取り出す
    pub out: &'a mut dyn Write,
    /// 今実行しているコードが属するモジュール。`@x` と関数名はここから引く。
    /// 最初は実行中の本体で、import したファイルや、そのファイルの関数を実行している間は切り替わる
    current: Rc<Module>,
    /// 本体のあるディレクトリ。モジュールの表示名をここからの相対パスにする
    root_dir: PathBuf,
    /// import 済みのファイル（正規化したパス → 状態）
    modules: HashMap<PathBuf, ModuleState>,
    /// 実行中の import の連なり（循環のエラー表示用）
    loading: Vec<Rc<str>>,
    frames: Vec<Frame>,
    natives: HashMap<&'static str, NativeFn>,
}

impl<'a> Interpreter<'a> {
    /// `base_dir` は、本体が `import` する相対パスの基準
    pub fn new(out: &'a mut dyn Write, base_dir: PathBuf) -> Self {
        Self {
            out,
            current: Rc::new(Module::new("".into(), base_dir.clone(), None)),
            root_dir: base_dir,
            modules: HashMap::new(),
            loading: Vec::new(),
            frames: Vec::new(),
            natives: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &'static str, f: NativeFn) {
        self.natives.insert(name, f);
    }

    pub fn native(&self, name: &str) -> Option<NativeFn> {
        self.natives.get(name).copied()
    }

    /// Resolver に渡す、定義済みの関数の情報
    pub fn callee(&self, name: &str) -> Option<Callee> {
        let arity = self
            .current
            .functions
            .borrow()
            .get(name)
            .map(|def| def.params.len());
        match arity {
            Some(arity) => Some(Callee::User { arity }),
            None => self.native_callee(name),
        }
    }

    /// 組み込み関数か。`import` は評価器が直接扱うが、Resolver からは組み込み関数に見える
    fn native_callee(&self, name: &str) -> Option<Callee> {
        (name == "import" || self.natives.contains_key(name)).then_some(Callee::Native)
    }

    pub fn get_var(&self, var: &Var) -> Option<Value> {
        match var {
            Var::Global(name) => self.current.globals.borrow().get(name).cloned(),
            Var::Local(name) => self.frames.last()?.locals.get(name).cloned(),
        }
    }

    /// `$x` は関数の中でしか使えないことを Resolver が保証している前提
    pub fn set_var(&mut self, var: &Var, value: Value) {
        match var {
            Var::Global(name) => self.current.globals.borrow_mut().set(name, value),
            Var::Local(name) => {
                let frame = self.frames.last_mut().expect("関数の外で $ 変数");
                frame.locals.set(name, value);
            }
        }
    }

    /// プログラムを実行する。最後の文が式なら、その値を返す（REPL で表示するため）。
    /// 何度呼んでも変数と関数は残る
    pub fn run(&mut self, program: &Program) -> RResult<Option<Value>> {
        // 定義より前の行から呼べるように、先に全関数を登録する（design.md 5.2）
        for stmt in &program.stmts {
            if let StmtKind::FuncDef(def) = &stmt.kind {
                let def = Rc::clone(def);
                self.current
                    .functions
                    .borrow_mut()
                    .insert(def.name.clone(), def);
            }
        }
        // トップレベルの `#` は Resolver が弾いているので、Flow は見なくてよい
        let Some((last, rest)) = program.stmts.split_last() else {
            return Ok(None);
        };
        self.exec_block(rest)?;
        if let StmtKind::Expr(expr) = &last.kind {
            return Ok(Some(self.eval(expr)?));
        }
        self.exec(last)?;
        Ok(None)
    }

    // ---- 文 ----

    fn exec_block(&mut self, block: &[Stmt]) -> RResult<Flow> {
        for stmt in block {
            let flow = self.exec(stmt)?;
            if !matches!(flow, Flow::Normal) {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, stmt: &Stmt) -> RResult<Flow> {
        match &stmt.kind {
            // 事前登録済み
            StmtKind::FuncDef(_) => {}
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                if self.eval_cond(cond)? {
                    return self.exec_block(then_block);
                }
                if let Some(block) = else_block {
                    return self.exec_block(block);
                }
            }
            StmtKind::While { cond, body } => {
                while self.eval_cond(cond)? {
                    match self.exec_block(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                    }
                }
            }
            StmtKind::Return(value) => {
                let value = match value {
                    Some(expr) => self.eval(expr)?,
                    None => Value::Nil,
                };
                return Ok(Flow::Return(value));
            }
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Assign {
                target,
                index: None,
                value,
            } => {
                let value = self.eval(value)?;
                self.set_var(target, value);
            }
            StmtKind::Assign {
                target,
                index: Some(index),
                value,
            } => {
                // 配列とマップは参照なので、clone しても同じものを書き換える
                let container = match self.get_var(target) {
                    Some(container) => container.clone(),
                    None => return Err(undefined(target, stmt.span)),
                };
                let key = self.eval(index)?;
                let value = self.eval(value)?;
                index_set(&container, &key, value, stmt.span, index.span)?;
            }
            StmtKind::Expr(expr) => {
                self.eval(expr)?;
            }
        }
        Ok(Flow::Normal)
    }

    /// `if` / `while` の条件。真偽値以外はエラー（design.md 5.3）
    fn eval_cond(&mut self, cond: &Expr) -> RResult<bool> {
        match self.eval(cond)? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::new(
                cond.span,
                format!(
                    "条件は真偽値である必要があります（{}でした）",
                    other.type_name()
                ),
            )),
        }
    }

    // ---- 式 ----

    fn eval(&mut self, expr: &Expr) -> RResult<Value> {
        let span = expr.span;
        let value = match &expr.kind {
            ExprKind::Number(n) => Value::Num(*n),
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Str(parts) => {
                let mut s = String::new();
                for part in parts {
                    match part {
                        StrPart::Text(text) => s.push_str(text),
                        StrPart::Var(var) => match self.get_var(var) {
                            Some(value) => s.push_str(&value.to_string()),
                            None => return Err(undefined(var, span)),
                        },
                    }
                }
                Value::Str(s.into())
            }
            ExprKind::Var(var) => match self.get_var(var) {
                Some(value) => value.clone(),
                None => return Err(undefined(var, span)),
            },
            ExprKind::Array(items) => {
                let values = items
                    .iter()
                    .map(|item| self.eval(item))
                    .collect::<RResult<Vec<_>>>()?;
                Value::Array(Rc::new(RefCell::new(values)))
            }
            ExprKind::Map(entries) => {
                let mut map = Map::default();
                for (key, value) in entries {
                    let k = self.eval(key)?;
                    let k = to_key(&k, key.span)?;
                    map.insert(k, self.eval(value)?);
                }
                Value::Map(Rc::new(RefCell::new(map)))
            }
            ExprKind::Member { target, name } => {
                let module = self.eval_module(target)?;
                let value = module.globals.borrow().get(name).cloned();
                value.ok_or_else(|| {
                    RuntimeError::new(
                        span,
                        format!("{} に変数 `@{name}` がありません", module_label(&module)),
                    )
                })?
            }
            ExprKind::MemberCall { target, name, args } => {
                let module = self.eval_module(target)?;
                let args = args
                    .iter()
                    .map(|arg| self.eval(arg))
                    .collect::<RResult<Vec<_>>>()?;
                let def = module.functions.borrow().get(name).cloned();
                match def {
                    Some(def) => self.call_user(&module, &def, args, span)?,
                    None => {
                        return Err(RuntimeError::new(
                            span,
                            format!("{} に関数 `{name}` がありません", module_label(&module)),
                        ));
                    }
                }
            }
            ExprKind::Index { target, index } => {
                let container = self.eval(target)?;
                let key = self.eval(index)?;
                index_get(&container, &key, target.span, index.span)?
            }
            ExprKind::Unary { op, operand } => match (op, self.eval(operand)?) {
                (UnaryOp::Neg, Value::Num(n)) => Value::Num(-n),
                (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
                (op, value) => {
                    return Err(RuntimeError::new(
                        span,
                        format!("{}に `{op}` は使えません", value.type_name()),
                    ));
                }
            },
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, span)?,
            ExprKind::Call { name, args } => self.call(name, args, span)?,
        };
        Ok(value)
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> RResult<Value> {
        use BinaryOp as B;

        // && と || は短絡評価する。両辺とも真偽値に限る
        if let B::And | B::Or = op {
            match (op, self.eval_logic_operand(lhs, op)?) {
                (B::Or, true) => return Ok(Value::Bool(true)),
                (B::And, false) => return Ok(Value::Bool(false)),
                _ => return Ok(Value::Bool(self.eval_logic_operand(rhs, op)?)),
            }
        }

        let l = self.eval(lhs)?;
        let r = self.eval(rhs)?;
        let value = match (op, &l, &r) {
            (B::Eq, _, _) => Value::Bool(l.equals(&r)),
            (B::NotEq, _, _) => Value::Bool(!l.equals(&r)),
            (B::Add, Value::Str(a), Value::Str(b)) => Value::Str(format!("{a}{b}").into()),
            (B::Div | B::Rem, Value::Num(_), Value::Num(b)) if *b == 0.0 => {
                return Err(RuntimeError::new(span, "0 で割ることはできません"));
            }
            (_, Value::Num(a), Value::Num(b)) => match op {
                B::Add => Value::Num(a + b),
                B::Sub => Value::Num(a - b),
                B::Mul => Value::Num(a * b),
                B::Div => Value::Num(a / b),
                B::Rem => Value::Num(a % b),
                _ => Value::Bool(compare(op, a.partial_cmp(b))),
            },
            (B::Lt | B::LtEq | B::Gt | B::GtEq, Value::Str(a), Value::Str(b)) => {
                Value::Bool(compare(op, Some(a.cmp(b))))
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    format!("{}と{}に `{op}` は使えません", l.type_name(), r.type_name()),
                ));
            }
        };
        Ok(value)
    }

    fn eval_logic_operand(&mut self, expr: &Expr, op: BinaryOp) -> RResult<bool> {
        match self.eval(expr)? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::new(
                expr.span,
                format!(
                    "`{op}` の両辺は真偽値である必要があります（{}でした）",
                    other.type_name()
                ),
            )),
        }
    }

    // ---- 関数呼び出し ----

    fn call(&mut self, name: &str, args: &[Expr], span: Span) -> RResult<Value> {
        let args = args
            .iter()
            .map(|arg| self.eval(arg))
            .collect::<RResult<Vec<_>>>()?;

        let def = self.current.functions.borrow().get(name).cloned();
        if let Some(def) = def {
            let module = Rc::clone(&self.current);
            return self.call_user(&module, &def, args, span);
        }
        if name == "import" {
            return self.import(&args, span);
        }
        match self.native(name) {
            Some(f) => f(self, &args).map_err(|message| RuntimeError::new(span, message)),
            None => Err(RuntimeError::new(span, format!("未定義の関数 `{name}`"))),
        }
    }

    /// `module` の関数 `def` を呼ぶ。実行している間は、`@x` と関数名を `module` から引く
    fn call_user(
        &mut self,
        module: &Rc<Module>,
        def: &FuncDef,
        args: Vec<Value>,
        span: Span,
    ) -> RResult<Value> {
        if args.len() != def.params.len() {
            return Err(RuntimeError::new(
                span,
                format!(
                    "`{}` の引数は {} 個です（{} 個渡されました）",
                    def.name,
                    def.params.len(),
                    args.len()
                ),
            ));
        }
        if self.frames.len() >= MAX_DEPTH {
            return Err(RuntimeError::new(
                span,
                format!(
                    "関数の呼び出しが深すぎます（{MAX_DEPTH} 段）。終わらない再帰になっていませんか"
                ),
            ));
        }

        let mut locals = Scope::default();
        for (param, value) in def.params.iter().zip(args) {
            locals.set(param, value);
        }
        self.frames.push(Frame {
            func_name: def.name.clone(),
            locals,
        });
        let caller = mem::replace(&mut self.current, Rc::clone(module));
        let result = self.exec_block(&def.body);
        self.current = caller;
        self.frames.pop();

        match result {
            Ok(Flow::Return(value)) => Ok(value),
            // break / continue が関数の外へ出ないことは Resolver が保証している
            Ok(Flow::Normal | Flow::Break | Flow::Continue) => Ok(Value::Nil),
            Err(mut e) => {
                // 別のファイルの関数で起きたエラーは、そのファイルの行を引用する
                if e.origin.is_none() {
                    e.origin = module.origin.clone();
                }
                e.trace.push(TraceEntry {
                    what: format!("{} の中", def.name),
                    span,
                    file: file_of(&self.current),
                });
                Err(e)
            }
        }
    }

    fn eval_module(&mut self, target: &Expr) -> RResult<Rc<Module>> {
        match self.eval(target)? {
            Value::Module(module) => Ok(module),
            other => Err(RuntimeError::new(
                target.span,
                format!(
                    "`.` を使えるのはモジュールだけです（{}でした）",
                    other.type_name()
                ),
            )),
        }
    }

    // ---- import（design.md 5.7） ----

    /// ファイルを読んで実行し、モジュールを返す。同じファイルは 1 回しか実行しない
    fn import(&mut self, args: &[Value], span: Span) -> RResult<Value> {
        let rel = match args {
            [Value::Str(rel)] => rel,
            [other] => {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "import の 1 番目の引数は文字列です（{}が渡されました）",
                        other.type_name()
                    ),
                ));
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    format!("import の引数は 1 個です（{} 個渡されました）", args.len()),
                ));
            }
        };

        let full = self.current.dir.join(&**rel);
        let shown = self.display_path(&full);
        let key = fs::canonicalize(&full).map_err(|e| {
            let reason = match e.kind() {
                io::ErrorKind::NotFound => "ファイルが見つかりません".to_string(),
                _ => e.to_string(),
            };
            RuntimeError::new(span, format!("`{rel}` を開けません: {reason}"))
        })?;
        match self.modules.get(&key) {
            Some(ModuleState::Loaded(module)) => return Ok(Value::Module(Rc::clone(module))),
            Some(ModuleState::Loading) => {
                let start = self.loading.iter().position(|n| *n == shown).unwrap_or(0);
                let mut chain: Vec<&str> = self.loading[start..].iter().map(|n| &**n).collect();
                chain.push(&shown);
                return Err(RuntimeError::new(
                    span,
                    format!("import が循環しています: {}", chain.join(" → ")),
                ));
            }
            None => {}
        }
        let src = fs::read_to_string(&full)
            .map_err(|e| RuntimeError::new(span, format!("`{rel}` を読み込めません: {e}")))?;

        let origin = Rc::new(Origin {
            path: shown.to_string(),
            src,
        });
        let entry = TraceEntry {
            what: format!("{shown} の読み込み中"),
            span,
            file: file_of(&self.current),
        };
        let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
        let module = Rc::new(Module::new(
            Rc::clone(&shown),
            dir,
            Some(Rc::clone(&origin)),
        ));

        // 読み込み中のエラーは、そのファイルの行を引用し、import した位置を呼び出し履歴に載せる
        let program = lexer::tokenize(&origin.src)
            .and_then(|tokens| parser::parse(&tokens))
            .and_then(|program| {
                resolver::resolve(&program, |name| self.native_callee(name))?;
                Ok(program)
            })
            .map_err(|diag| RuntimeError {
                message: diag.message,
                span: diag.span,
                trace: vec![entry.clone()],
                origin: Some(Rc::clone(&origin)),
            })?;

        self.modules.insert(key.clone(), ModuleState::Loading);
        self.loading.push(shown);
        let importer = mem::replace(&mut self.current, Rc::clone(&module));
        let result = self.run(&program);
        self.current = importer;
        self.loading.pop();

        match result {
            Ok(_) => {
                self.modules
                    .insert(key, ModuleState::Loaded(Rc::clone(&module)));
                Ok(Value::Module(module))
            }
            Err(mut e) => {
                // 失敗したファイルは、直して再実行できるよう覚えない
                self.modules.remove(&key);
                if e.origin.is_none() {
                    e.origin = Some(origin);
                }
                e.trace.push(entry);
                Err(e)
            }
        }
    }

    /// モジュールの表示名。本体のあるディレクトリからの相対パスにする
    fn display_path(&self, full: &Path) -> Rc<str> {
        let path = full.strip_prefix(&self.root_dir).unwrap_or(full);
        path.to_string_lossy().replace('\\', "/").into()
    }
}

/// 呼び出し履歴に載せる、ファイルの名前。実行中の本体は None
fn file_of(module: &Module) -> Option<Rc<str>> {
    (!module.name.is_empty()).then(|| Rc::clone(&module.name))
}

fn module_label(module: &Module) -> String {
    format!("モジュール {:?}", &*module.name)
}

fn undefined(var: &Var, span: Span) -> RuntimeError {
    RuntimeError::new(span, format!("未定義の変数 `{var}`"))
}

/// `container[key]` を読む。配列なら番号、マップなら文字列のキー
fn index_get(container: &Value, key: &Value, target_span: Span, key_span: Span) -> RResult<Value> {
    match container {
        Value::Array(items) => {
            let items = items.borrow();
            let i = to_index(key, items.len(), key_span)?;
            Ok(items[i].clone())
        }
        Value::Map(map) => {
            let k = to_key(key, key_span)?;
            map.borrow()
                .get(&k)
                .cloned()
                .ok_or_else(|| RuntimeError::new(key_span, format!("キー {k:?} がありません")))
        }
        other => Err(not_container(other, target_span)),
    }
}

/// `container[key] = value`。マップにキーがなければ追加する
fn index_set(
    container: &Value,
    key: &Value,
    value: Value,
    target_span: Span,
    key_span: Span,
) -> RResult<()> {
    match container {
        Value::Array(items) => {
            let mut items = items.borrow_mut();
            let i = to_index(key, items.len(), key_span)?;
            items[i] = value;
        }
        Value::Map(map) => {
            let k = to_key(key, key_span)?;
            map.borrow_mut().insert(k, value);
        }
        other => return Err(not_container(other, target_span)),
    }
    Ok(())
}

fn not_container(value: &Value, span: Span) -> RuntimeError {
    RuntimeError::new(
        span,
        format!(
            "添字を使えるのは配列とマップだけです（{}でした）",
            value.type_name()
        ),
    )
}

/// マップのキーとして使える値（文字列）か確かめる（design.md 5.4）
fn to_key(value: &Value, span: Span) -> RResult<Rc<str>> {
    match value {
        Value::Str(s) => Ok(Rc::clone(s)),
        other => Err(RuntimeError::new(
            span,
            format!(
                "マップのキーは文字列である必要があります（{}でした）",
                other.type_name()
            ),
        )),
    }
}

/// 添字として使える値（0 以上 len 未満の整数）か確かめる
fn to_index(value: &Value, len: usize, span: Span) -> RResult<usize> {
    let Value::Num(n) = *value else {
        return Err(RuntimeError::new(
            span,
            format!(
                "添字は数値である必要があります（{}でした）",
                value.type_name()
            ),
        ));
    };
    if n.fract() != 0.0 || n < 0.0 || n >= len as f64 {
        return Err(RuntimeError::new(
            span,
            format!("添字 {n} は範囲外です（要素は {len} 個）"),
        ));
    }
    Ok(n as usize)
}

fn compare(op: BinaryOp, ord: Option<Ordering>) -> bool {
    // NaN との比較は常に false
    let Some(ord) = ord else {
        return false;
    };
    match op {
        BinaryOp::Lt => ord.is_lt(),
        BinaryOp::LtEq => ord.is_le(),
        BinaryOp::Gt => ord.is_gt(),
        BinaryOp::GtEq => ord.is_ge(),
        _ => unreachable!("比較演算子ではない: {op}"),
    }
}
