//! AST の tree-walking 評価器

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::env::{Env, Frame};
use super::error::{RuntimeError, TraceEntry};
use super::gc;
use super::map::Map;
use super::module::Module;
use super::value::{Closure, NativeRef, Value};
use crate::diag::Origin;
use crate::sema::resolver::{self, Callee};
use crate::syntax::ast::{
    BinaryOp, Expr, ExprKind, FuncDef, Program, Stmt, StmtKind, UnaryOp, Var,
};
use crate::syntax::lexer::StrPart;
use crate::syntax::span::Span;
use crate::syntax::{lexer, parser};

/// 組み込みモジュールの定数の値
#[derive(Debug, Clone, Copy)]
pub enum Const {
    Num(f64),
    Str(&'static str),
}

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
    /// スクリプトに渡されたコマンドライン引数（`std:os` の `args`）
    pub args: Vec<String>,
    /// `os.exit` が呼ばれたときの終了コード。Some なら実行を止める
    pub exit_code: Option<i32>,
    /// 今実行しているコードが属するモジュール。関数名はここから引く。
    /// 最初は実行中の本体で、import したファイルや、そのファイルの関数を実行している間は切り替わる
    current: Rc<Module>,
    /// いま入っているスコープ。変数はここから外側へ向かって探す。
    /// `( )` のブロックと関数呼び出しごとに内側のスコープへ入り、出るときに戻す
    env: Rc<Env>,
    /// 本体のあるディレクトリ。モジュールの表示名をここからの相対パスにする
    root_dir: PathBuf,
    /// import 済みのファイル（正規化したパス → 状態）
    modules: HashMap<PathBuf, ModuleState>,
    /// 実行中の import の連なり（循環のエラー表示用）
    loading: Vec<Rc<str>>,
    frames: Vec<Frame>,
    natives: HashMap<&'static str, NativeFn>,
    /// いま実行している組み込み関数の呼び出し位置。組み込み関数が関数値を呼ぶときの位置になる
    native_span: Span,
    /// 組み込み関数が呼んだ関数値の中で起きたエラー。`call_value` が預け、
    /// 組み込み関数がエラーを返したときに `call_native` が取り出す（呼び出し履歴を保つため）
    native_error: Option<RuntimeError>,
    /// 関数の中を実行しているか。`?` がエラーを返せる場所かどうか（import したファイルの
    /// トップレベルは、呼び出し元が関数の中でも「関数の外」）
    in_func: bool,
    /// 組み込みモジュールの登録（`std:json` → 関数の表）と、import 済みのもの
    std_registry: HashMap<&'static str, Vec<(&'static str, NativeFn)>>,
    std_loaded: HashMap<&'static str, Rc<Module>>,
    /// 組み込みモジュールの定数（`std:math` の `pi` など）
    std_consts: HashMap<&'static str, Vec<(&'static str, Const)>>,
    /// `?` が関数から返そうとしているエラー値（`RuntimeError::early_return` と組）
    returning: Option<Value>,
}

impl<'a> Interpreter<'a> {
    /// `base_dir` は、本体が `import` する相対パスの基準
    pub fn new(out: &'a mut dyn Write, base_dir: PathBuf) -> Self {
        let current = Rc::new(Module::new("".into(), base_dir.clone(), None));
        Self {
            out,
            args: Vec::new(),
            exit_code: None,
            env: Rc::clone(&current.globals),
            current,
            root_dir: base_dir,
            modules: HashMap::new(),
            loading: Vec::new(),
            frames: Vec::new(),
            natives: HashMap::new(),
            native_span: Span { line: 1, col: 1 },
            native_error: None,
            in_func: false,
            std_registry: HashMap::new(),
            std_loaded: HashMap::new(),
            std_consts: HashMap::new(),
            returning: None,
        }
    }

    pub fn register(&mut self, name: &'static str, f: NativeFn) {
        self.natives.insert(name, f);
    }

    /// 組み込みモジュールを登録する。`name` は `std:json`、関数の名前は `json.parse` のように
    /// モジュール名をつけて書き、`.` より後ろが `@j.parse[...]` で呼ぶときの名前になる
    pub fn register_module(&mut self, name: &'static str, functions: &[(&'static str, NativeFn)]) {
        self.std_registry.insert(name, functions.to_vec());
    }

    /// 組み込みモジュールの定数（数値）を登録する。モジュールの変数として読める
    pub fn register_module_constants(
        &mut self,
        name: &'static str,
        constants: &[(&'static str, Const)],
    ) {
        self.std_consts.insert(name, constants.to_vec());
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

    /// Resolver に渡す、前の入力（REPL）で宣言したトップレベルの変数があるか
    pub fn has_global(&self, name: &str) -> bool {
        self.current.globals.get_own(name).is_some()
    }

    pub fn get_var(&self, var: &Var) -> Option<Value> {
        self.env.get(&var.0)
    }

    /// プログラムを実行する。最後の文が式なら、その値を返す（REPL で表示するため）。
    /// 何度呼んでも変数と関数は残る
    ///
    /// `keep_last` が false なら、最後の文が式でも値は捨てる（ファイルの実行。
    /// 捨てられたエラー値は、ほかの文と同じく実行時エラーになる）
    pub fn run(&mut self, program: &Program, keep_last: bool) -> RResult<Option<Value>> {
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
        debug_assert!(Rc::ptr_eq(&self.env, &self.current.globals));
        // トップレベルの `#` は Resolver が弾いているので、Flow は見なくてよい
        let Some((last, rest)) = program.stmts.split_last() else {
            return Ok(None);
        };
        self.exec_block(rest)?;
        if keep_last && let StmtKind::Expr(expr) = &last.kind {
            return Ok(Some(self.eval(expr)?));
        }
        self.exec(last)?;
        Ok(None)
    }

    // ---- 文 ----

    /// 新しいスコープに入ってブロックを実行する。エラーでも元のスコープに戻す
    fn exec_scoped(&mut self, block: &[Stmt]) -> RResult<Flow> {
        let inner = Env::new(Some(Rc::clone(&self.env)));
        let outer = mem::replace(&mut self.env, inner);
        let result = self.exec_block(block);
        self.env = outer;
        result
    }

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
                    return self.exec_scoped(then_block);
                }
                if let Some(block) = else_block {
                    return self.exec_scoped(block);
                }
            }
            StmtKind::While { cond, body } => {
                while self.eval_cond(cond)? {
                    gc::maybe_collect();
                    match self.exec_scoped(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                    }
                }
            }
            StmtKind::For { var, iter, body } => {
                let items = match self.eval(iter)? {
                    // 回し始めた時点の要素を使う
                    Value::Array(items) => items.borrow().clone(),
                    other => {
                        return Err(RuntimeError::new(
                            iter.span,
                            format!(
                                "for で回せるのは配列だけです（{}でした）。マップは `map_keys` でキーの配列にします",
                                other.type_name()
                            ),
                        ));
                    }
                };
                for item in items {
                    gc::maybe_collect();
                    // ループ変数と本体は、繰り返しごとに新しい同じスコープ
                    let inner = Env::new(Some(Rc::clone(&self.env)));
                    inner.declare(&var.0, item);
                    let outer = mem::replace(&mut self.env, inner);
                    let flow = self.exec_block(body);
                    self.env = outer;
                    match flow? {
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
            StmtKind::Declare { target, value } => {
                let value = self.eval(value)?;
                self.env.declare(&target.0, value);
            }
            StmtKind::Assign {
                target,
                indices,
                value,
            } => {
                let Some((last, outer)) = indices.split_last() else {
                    let value = self.eval(value)?;
                    if !self.env.assign(&target.0, value) {
                        return Err(undefined(target, stmt.span));
                    }
                    return Ok(Flow::Normal);
                };
                // 配列とマップは参照なので、clone しても同じものを書き換える。
                // `@m{"a"}{"b"} = v` は、外側の添字を読んでいって最後の 1 つだけ書く
                let mut container = match self.get_var(target) {
                    Some(container) => container,
                    None => return Err(undefined(target, stmt.span)),
                };
                for index in outer {
                    let key = self.eval(index)?;
                    container = index_get(&container, &key, stmt.span, index.span)?;
                }
                let key = self.eval(last)?;
                let value = self.eval(value)?;
                index_set(&container, &key, value, stmt.span, last.span)?;
            }
            StmtKind::Expr(expr) => {
                // 捨てられたエラー値は、失敗に気づかないまま進まないよう実行時エラーにする
                if let Value::Error(e) = self.eval(expr)? {
                    return Err(RuntimeError::new(
                        expr.span,
                        format!(
                            "エラー値が捨てられました（{}: {}）。変数に入れるか、`?` か `is_error` で扱ってください",
                            e.kind, e.message
                        ),
                    ));
                }
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
            ExprKind::Nil => Value::Nil,
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
                gc::new_array(values)
            }
            ExprKind::Map(entries) => {
                let mut map = Map::default();
                for (key, value) in entries {
                    let k = self.eval(key)?;
                    let k = to_key(&k, key.span)?;
                    map.insert(k, self.eval(value)?);
                }
                gc::new_map(map)
            }
            ExprKind::Member { target, name } => {
                let module = self.eval_module(target)?;
                // 変数がなければ、同じ名前の関数を関数値として返す（design.md 5.8）
                let value = module.globals.get_own(name).or_else(|| {
                    if let Some(native) = module.natives.get(name.as_str()) {
                        return Some(Value::Native(native.clone()));
                    }
                    let def = module.functions.borrow().get(name).cloned()?;
                    Some(Value::Func(Closure::register(Closure {
                        def,
                        env: Rc::clone(&module.globals),
                        module: Rc::clone(&module),
                    })))
                });
                value.ok_or_else(|| {
                    RuntimeError::new(
                        span,
                        format!("{} に変数 `@{name}` がありません", module_label(&module)),
                    )
                })?
            }
            ExprKind::FuncRef(name) => self.func_value(name, span)?,
            ExprKind::Try(inner) => match self.eval(inner)? {
                Value::Error(e) if self.in_func => {
                    // いまの関数からエラーを返す。呼び出し（call_def）が受け取る
                    self.returning = Some(Value::Error(e));
                    let mut signal = RuntimeError::new(span, "`?` による戻り");
                    signal.early_return = true;
                    return Err(signal);
                }
                // 関数の外では、エラーで止まる
                Value::Error(e) => return Err(RuntimeError::new(span, &*e.message)),
                value => value,
            },
            ExprKind::Lambda(def) => Value::Func(Closure::register(Closure {
                def: Rc::clone(def),
                env: Rc::clone(&self.env),
                module: Rc::clone(&self.current),
            })),
            ExprKind::CallValue { callee, args } => {
                let f = self.eval(callee)?;
                let args = args
                    .iter()
                    .map(|arg| self.eval(arg))
                    .collect::<RResult<Vec<_>>>()?;
                self.call_function(&f, args, span, callee.span)?
            }
            ExprKind::MemberCall { target, name, args } => {
                let module = self.eval_module(target)?;
                let args = args
                    .iter()
                    .map(|arg| self.eval(arg))
                    .collect::<RResult<Vec<_>>>()?;
                let def = module.functions.borrow().get(name).cloned();
                match (def, module.natives.get(name.as_str())) {
                    (Some(def), _) => self.call_user(&module, &def, args, span)?,
                    (None, Some(native)) => self.call_native(native.f, &args, span)?,
                    (None, None) => {
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
            Some(f) => self.call_native(f, &args, span),
            None => Err(RuntimeError::new(span, format!("未定義の関数 `{name}`"))),
        }
    }

    /// 組み込み関数を呼ぶ。中で呼んだ関数値がエラーになっていたら、そのエラーをそのまま返す
    fn call_native(&mut self, f: NativeFn, args: &[Value], span: Span) -> RResult<Value> {
        let saved = mem::replace(&mut self.native_span, span);
        let result = f(self, args);
        self.native_span = saved;
        match result {
            Ok(value) => {
                self.native_error = None;
                Ok(value)
            }
            Err(message) => {
                if self.exit_code.is_some() {
                    let mut signal = RuntimeError::new(span, "os.exit による終了");
                    signal.exit = true;
                    return Err(signal);
                }
                Err(self
                    .native_error
                    .take()
                    .unwrap_or_else(|| RuntimeError::new(span, message)))
            }
        }
    }

    /// 関数値を呼ぶ。`span` は呼び出し位置、`callee_span` は呼ぶ式の位置
    fn call_function(
        &mut self,
        f: &Value,
        args: Vec<Value>,
        span: Span,
        callee_span: Span,
    ) -> RResult<Value> {
        match f {
            Value::Func(closure) => {
                self.call_def(&closure.module, &closure.def, &closure.env, args, span)
            }
            Value::Native(native) => self.call_native(native.f, &args, span),
            other => {
                let hint = match other {
                    Value::Array(_) | Value::Map(_) => {
                        "。配列やマップの要素は `@a{0}` のように `{ }` で読みます"
                    }
                    _ => "",
                };
                Err(RuntimeError::new(
                    callee_span,
                    format!(
                        "`[ ]` で呼び出せるのは関数だけです（{}でした）{hint}",
                        other.type_name()
                    ),
                ))
            }
        }
    }

    /// `call_value` が預けたエラー（位置と呼び出し履歴つき）を取り出す
    pub fn take_native_error(&mut self) -> Option<RuntimeError> {
        self.native_error.take()
    }

    /// 組み込み関数から関数値を呼ぶ（`arr_map` など）。呼ぶ関数の中で起きたエラーは、
    /// 呼び出し履歴つきで `native_error` に預け、メッセージだけを返す
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>) -> Result<Value, String> {
        let span = self.native_span;
        self.call_function(f, args, span, span).map_err(|e| {
            let message = e.message.clone();
            self.native_error = Some(e);
            message
        })
    }

    /// 関数名だけを書いたときの関数値（`@f := add`）。ファイルの関数、組み込み関数の順に探す
    fn func_value(&self, name: &str, span: Span) -> RResult<Value> {
        let def = self.current.functions.borrow().get(name).cloned();
        if let Some(def) = def {
            return Ok(Value::Func(Closure::register(Closure {
                def,
                env: Rc::clone(&self.current.globals),
                module: Rc::clone(&self.current),
            })));
        }
        if name == "import" {
            return Err(RuntimeError::new(span, "`import` は関数値にできません"));
        }
        match self.natives.get_key_value(name) {
            Some((name, f)) => Ok(Value::Native(NativeRef { name, f: *f })),
            None => Err(RuntimeError::new(span, format!("未定義の関数 `{name}`"))),
        }
    }

    /// `module` の名前つきの関数 `def` を呼ぶ
    fn call_user(
        &mut self,
        module: &Rc<Module>,
        def: &FuncDef,
        args: Vec<Value>,
        span: Span,
    ) -> RResult<Value> {
        let env = Rc::clone(&module.globals);
        self.call_def(module, def, &env, args, span)
    }

    /// 関数 `def` を呼ぶ。`parent` は関数が作られた場所のスコープで、その内側に引数のスコープを作る。
    /// 実行している間は、関数名を `module` から引く
    fn call_def(
        &mut self,
        module: &Rc<Module>,
        def: &FuncDef,
        parent: &Rc<Env>,
        args: Vec<Value>,
        span: Span,
    ) -> RResult<Value> {
        let name = func_label(def);
        if args.len() != def.params.len() {
            return Err(RuntimeError::new(
                span,
                format!(
                    "{} の引数は {} 個です（{} 個渡されました）",
                    quoted_label(def),
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

        gc::maybe_collect();
        // 関数のスコープは、そのファイルのトップレベルのスコープの内側に作る
        let env = Env::new(Some(Rc::clone(parent)));
        for (param, value) in def.params.iter().zip(args) {
            env.declare(param, value);
        }
        self.frames.push(Frame {
            func_name: name.to_string(),
        });
        let caller = mem::replace(&mut self.current, Rc::clone(module));
        let caller_env = mem::replace(&mut self.env, env);
        let was_in_func = mem::replace(&mut self.in_func, true);
        let result = self.exec_block(&def.body);
        self.in_func = was_in_func;
        self.env = caller_env;
        self.current = caller;
        self.frames.pop();

        match result {
            // `?` が返したエラー値が、この呼び出しの戻り値になる
            Err(e) if e.early_return => Ok(self.returning.take().unwrap_or(Value::Nil)),
            Ok(Flow::Return(value)) => Ok(value),
            // break / continue が関数の外へ出ないことは Resolver が保証している
            Ok(Flow::Normal | Flow::Break | Flow::Continue) => Ok(Value::Nil),
            Err(mut e) => {
                // 別のファイルの関数で起きたエラーは、そのファイルの行を引用する
                if e.origin.is_none() {
                    e.origin = module.origin.clone();
                }
                e.trace.push(TraceEntry {
                    what: format!("{name} の中"),
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

        if rel.starts_with("std:") {
            return self.import_std(rel, span);
        }
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
                resolver::resolve(&program, |name| self.native_callee(name), |_| false)?;
                Ok(program)
            })
            .map_err(|diag| RuntimeError {
                message: diag.message,
                span: diag.span,
                trace: vec![entry.clone()],
                origin: Some(Rc::clone(&origin)),
                early_return: false,
                exit: false,
            })?;

        self.modules.insert(key.clone(), ModuleState::Loading);
        self.loading.push(shown);
        let importer = mem::replace(&mut self.current, Rc::clone(&module));
        let importer_env = mem::replace(&mut self.env, Rc::clone(&module.globals));
        let importer_in_func = mem::replace(&mut self.in_func, false);
        let result = self.run(&program, false);
        self.in_func = importer_in_func;
        self.env = importer_env;
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

    /// 組み込みモジュール（`std:json` など）。同じものは同じモジュールを返す
    fn import_std(&mut self, name: &str, span: Span) -> RResult<Value> {
        let Some((&key, functions)) = self.std_registry.get_key_value(name) else {
            let missing_feature = match name {
                "std:http" if !cfg!(feature = "http") => Some("http"),
                "std:crypto" if !cfg!(feature = "crypto") => Some("crypto"),
                _ => None,
            };
            if let Some(feature) = missing_feature {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "この版には `{name}` が含まれていません（`{feature}` feature を付けてビルドしてください）"
                    ),
                ));
            }
            let mut names: Vec<&str> = self.std_registry.keys().copied().collect();
            names.sort_unstable();
            return Err(RuntimeError::new(
                span,
                format!(
                    "組み込みモジュール `{name}` はありません（あるのは {}）",
                    names.join("、")
                ),
            ));
        };
        if let Some(module) = self.std_loaded.get(key) {
            return Ok(Value::Module(Rc::clone(module)));
        }
        let mut module = Module::new(key.into(), PathBuf::new(), None);
        for &(full, f) in functions {
            let short = full.rsplit('.').next().unwrap_or(full);
            module.natives.insert(short, NativeRef { name: full, f });
        }
        for &(name, value) in self.std_consts.get(key).into_iter().flatten() {
            let value = match value {
                Const::Num(n) => Value::Num(n),
                Const::Str(s) => Value::Str(s.into()),
            };
            module.globals.declare(name, value);
        }
        let module = Rc::new(module);
        self.std_loaded.insert(key, Rc::clone(&module));
        Ok(Value::Module(module))
    }

    /// モジュールの表示名。本体のあるディレクトリからの相対パスにする
    fn display_path(&self, full: &Path) -> Rc<str> {
        let path = full.strip_prefix(&self.root_dir).unwrap_or(full);
        path.to_string_lossy().replace('\\', "/").into()
    }
}

/// エラー表示用の関数名。無名関数は「無名関数」
fn func_label(def: &FuncDef) -> &str {
    if def.name.is_empty() {
        "無名関数"
    } else {
        &def.name
    }
}

/// エラーメッセージの文中で使う関数名。名前つきなら `` `add` ``、無名関数は「無名関数」
fn quoted_label(def: &FuncDef) -> String {
    if def.name.is_empty() {
        "無名関数".to_string()
    } else {
        format!("`{}`", def.name)
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

/// `container{key}` を読む。配列なら番号、マップなら文字列のキー
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
        Value::Bytes(bytes) => {
            let i = to_index(key, bytes.len(), key_span)?;
            Ok(Value::Num(f64::from(bytes[i])))
        }
        other => Err(not_container(other, target_span)),
    }
}

/// `container{key} = value`。マップにキーがなければ追加する
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
        Value::Bytes(_) => {
            return Err(RuntimeError::new(
                target_span,
                "バイト列は書き換えられません（作り直してください）",
            ));
        }
        other => return Err(not_container(other, target_span)),
    }
    Ok(())
}

fn not_container(value: &Value, span: Span) -> RuntimeError {
    RuntimeError::new(
        span,
        format!(
            "添字を使えるのは配列・マップ・バイト列だけです（{}でした）",
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
