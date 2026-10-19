//! バイトコード VM（design.md 5.45）
//!
//! 関数の呼び出し 1 回分の枠（`VmFrame`）と値のスタックをヒープに持つ。Dolphin の関数を呼ぶとき
//! Rust の再帰を使わない。組み込み関数が関数値を呼ぶ（`call_value`）ときだけ、入れ子で `run_vm` が動く。
//!
//! 変数の置き場所は評価器のときと同じ（名前で引く `Env` の連なり）。`Interpreter` の
//! `current`（いまのモジュール）と `env`（いまのスコープ）は、いま動いている枠のものを指し続ける。
//! 枠を作るときに呼び出し元の分を枠に預け、枠を抜けるときに戻す

use std::cell::RefCell;
use std::mem;
use std::rc::Rc;

use super::{
    Closure, Env, FuncDef, Interpreter, Map, Module, RResult, RuntimeError, TraceEntry, Value,
    file_of, func_label, index_get, index_set, int_overflow, module_label, not_callable,
    quoted_label, reject_module, to_key, undefined_name,
};
use crate::runtime::bytecode::{Chunk, Op};
use crate::runtime::compile::compile_function;
use crate::runtime::gc;
use crate::sema::resolver;
use crate::syntax::ast::{BinaryOp, Pattern, UnaryOp};
use crate::syntax::span::Span;

/// 枠の種類
pub(super) enum FrameKind {
    /// 関数の呼び出し。`def` は呼び出し履歴の名前を作るため、`call_span` は呼び出し位置
    Function { def: Rc<FuncDef>, call_span: Span },
    /// ファイルの本体（`import` したファイルを含む）
    Program,
}

/// 関数の呼び出し 1 回分（またはファイルの本体 1 つ分）の状態
pub(super) struct VmFrame {
    chunk: Rc<Chunk>,
    ip: usize,
    /// 値のスタックで、この枠が使い始める位置
    base: usize,
    kind: FrameKind,
    /// この枠のコードが属するモジュール
    module: Rc<Module>,
    /// 渡された引数の数（既定値を評価するか決める）
    given: usize,
    /// `PushScope` で入る前のスコープ（`PopScope` で戻る）
    scopes: Vec<Rc<Env>>,
    /// 呼び出し元のモジュールとスコープ。枠を抜けるときに戻す
    caller_module: Rc<Module>,
    caller_env: Rc<Env>,
}

/// 1 命令を実行した後にどうするか
enum Step {
    Next,
    /// 枠が変わった（呼び出しか戻り）。枠から読み直す
    Reload,
    /// 入口の枠が戻った
    Done(Value),
}

impl Interpreter<'_> {
    /// 関数の命令列。最初に呼ばれたときにコンパイルして覚える
    fn chunk_for(&mut self, def: &Rc<FuncDef>) -> Rc<Chunk> {
        let key = Rc::as_ptr(def) as usize;
        if let Some((_, chunk)) = self.chunks.get(&key) {
            return Rc::clone(chunk);
        }
        let chunk = Rc::new(compile_function(def));
        // 定義を持たせておく（解放されて、同じアドレスが別の定義に使われないように）
        self.chunks.insert(key, (Rc::clone(def), Rc::clone(&chunk)));
        chunk
    }

    /// ファイルの本体の枠を積む。スコープは変えない（いまのスコープがファイルのトップレベル）
    pub(super) fn push_program_frame(&mut self, chunk: Chunk) {
        self.vm_frames.push(VmFrame {
            chunk: Rc::new(chunk),
            ip: 0,
            base: self.stack.len(),
            kind: FrameKind::Program,
            module: Rc::clone(&self.current),
            given: 0,
            scopes: Vec::new(),
            caller_module: Rc::clone(&self.current),
            caller_env: Rc::clone(&self.env),
        });
    }

    /// 関数 `def` を呼ぶ枠を積む。`parent` は関数が作られた場所のスコープで、その内側に引数のスコープを作る。
    /// 実行している間は、関数名を `module` から引く
    pub(super) fn push_function_frame(
        &mut self,
        module: &Rc<Module>,
        def: &Rc<FuncDef>,
        parent: &Rc<Env>,
        args: Vec<Value>,
        span: Span,
    ) -> RResult<()> {
        if args.len() < def.min_args() || def.max_args().is_some_and(|max| args.len() > max) {
            return Err(RuntimeError::new(
                span,
                resolver::arity_message(
                    &quoted_label(def),
                    def.min_args(),
                    def.max_args(),
                    args.len(),
                ),
            ));
        }
        if self.depth >= super::MAX_DEPTH {
            return Err(RuntimeError::new(
                span,
                format!(
                    "関数の呼び出しが深すぎます（{} 段）。終わらない再帰になっていませんか",
                    super::MAX_DEPTH
                ),
            ));
        }

        gc::maybe_collect();
        // 関数のスコープは、そのファイルのトップレベルのスコープの内側に作る
        let env = Env::new(Some(Rc::clone(parent)));
        let given = args.len().min(def.params.len());
        let mut args = args;
        let extra = args.split_off(given);
        for (param, value) in def.params.iter().zip(args) {
            env.declare(&param.name, value);
        }
        if let Some(rest) = &def.rest {
            env.declare(rest, gc::new_array(extra));
        }
        let chunk = self.chunk_for(def);
        let caller_module = mem::replace(&mut self.current, Rc::clone(module));
        let caller_env = mem::replace(&mut self.env, env);
        self.depth += 1;
        self.vm_frames.push(VmFrame {
            chunk,
            ip: 0,
            base: self.stack.len(),
            kind: FrameKind::Function {
                def: Rc::clone(def),
                call_span: span,
            },
            module: Rc::clone(module),
            given,
            scopes: Vec::new(),
            caller_module,
            caller_env,
        });
        Ok(())
    }

    /// 枠を 1 つ抜けて、呼び出し元の状態に戻す
    fn pop_frame(&mut self) -> VmFrame {
        let frame = self.vm_frames.pop().expect("抜ける枠がある");
        self.stack.truncate(frame.base);
        self.current = Rc::clone(&frame.caller_module);
        self.env = Rc::clone(&frame.caller_env);
        if let FrameKind::Function { .. } = frame.kind {
            self.depth -= 1;
        }
        frame
    }

    /// エラーのために、`target` の枠まで（`target` は残す）枠を抜ける。別のファイルの関数で起きた
    /// エラーはそのファイルの行を引用し、呼び出し履歴に「f の中」を足す
    fn unwind(&mut self, mut e: RuntimeError, target: usize) -> RuntimeError {
        while self.vm_frames.len() > target {
            let frame = self.pop_frame();
            if let FrameKind::Function { def, call_span } = frame.kind {
                if e.origin.is_none() {
                    e.origin = frame.module.origin.clone();
                }
                e.trace.push(TraceEntry {
                    what: format!("{} の中", func_label(&def)),
                    span: call_span,
                    file: file_of(&self.current),
                });
            }
        }
        e
    }

    /// `target` 枚の枠が残るまで（積んだ入口の枠が戻るまで）命令を実行して、その戻り値を返す
    pub(super) fn run_vm(&mut self, target: usize) -> RResult<Value> {
        loop {
            let (chunk, mut ip) = {
                let frame = self.vm_frames.last().expect("実行する枠がある");
                (Rc::clone(&frame.chunk), frame.ip)
            };
            loop {
                let op = &chunk.code[ip];
                let span = chunk.spans[ip];
                ip += 1;
                match self.step(op, span, &chunk, &mut ip, target) {
                    Ok(Step::Next) => {}
                    Ok(Step::Reload) => break,
                    Ok(Step::Done(value)) => return Ok(value),
                    Err(e) => return Err(self.unwind(e, target)),
                }
            }
        }
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("スタックに値がある")
    }

    fn split_top(&mut self, n: usize) -> Vec<Value> {
        let at = self.stack.len() - n;
        self.stack.split_off(at)
    }

    /// いまの枠の続きの位置を覚える（別の枠へ移る前に）
    fn save_ip(&mut self, ip: usize) {
        if let Some(frame) = self.vm_frames.last_mut() {
            frame.ip = ip;
        }
    }

    fn do_return(&mut self, value: Value, target: usize) -> Step {
        self.pop_frame();
        if self.vm_frames.len() == target {
            Step::Done(value)
        } else {
            self.stack.push(value);
            Step::Reload
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(
        &mut self,
        op: &Op,
        span: Span,
        chunk: &Rc<Chunk>,
        ip: &mut usize,
        target: usize,
    ) -> RResult<Step> {
        match op {
            Op::Const(i) => self.stack.push(chunk.consts[*i as usize].clone()),
            Op::LoadVar(n) => {
                let name = &chunk.names[*n as usize];
                match self.env.get(name) {
                    Some(value) => self.stack.push(value),
                    None => return Err(undefined_name(name, span)),
                }
            }
            Op::Declare(n) => {
                let value = self.pop();
                self.env.declare(&chunk.names[*n as usize], value);
            }
            Op::AssignVar(n) => {
                let value = self.pop();
                let name = &chunk.names[*n as usize];
                if !self.env.assign(name, value) {
                    return Err(undefined_name(name, span));
                }
            }
            Op::Pop => {
                self.pop();
            }
            Op::PopCheckError => {
                // 捨てられたエラー値は、失敗に気づかないまま進まないよう実行時エラーにする
                if let Value::Error(e) = self.pop() {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "エラー値が捨てられました（{}: {}）。変数に入れるか、`?` か `is_error` で扱ってください",
                            e.kind, e.message
                        ),
                    ));
                }
            }
            Op::PushScope => {
                let inner = Env::new(Some(Rc::clone(&self.env)));
                let outer = mem::replace(&mut self.env, inner);
                self.vm_frames
                    .last_mut()
                    .expect("枠がある")
                    .scopes
                    .push(outer);
            }
            Op::PopScope => {
                let outer = self
                    .vm_frames
                    .last_mut()
                    .expect("枠がある")
                    .scopes
                    .pop()
                    .expect("閉じるスコープがある");
                self.env = outer;
            }
            Op::Jump(t) => *ip = *t as usize,
            Op::JumpIfFalseCond(t) => match self.pop() {
                Value::Bool(true) => {}
                Value::Bool(false) => *ip = *t as usize,
                other => {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "条件は真偽値である必要があります（{}でした）",
                            other.type_name()
                        ),
                    ));
                }
            },
            Op::LoopTick => gc::maybe_collect(),
            Op::LogicOperand(op) => {
                let top = self.stack.last().expect("スタックに値がある");
                if !matches!(top, Value::Bool(_)) {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "`{op}` の両辺は真偽値である必要があります（{}でした）",
                            top.type_name()
                        ),
                    ));
                }
            }
            Op::ShortCircuit(op, t) => {
                let short = matches!(
                    (op, self.stack.last()),
                    (BinaryOp::And, Some(Value::Bool(false)))
                        | (BinaryOp::Or, Some(Value::Bool(true)))
                );
                if short {
                    *ip = *t as usize;
                } else {
                    self.pop();
                }
            }
            Op::Binary(op) => {
                let r = self.pop();
                let l = self.pop();
                self.stack.push(binary_values(*op, &l, &r, span)?);
            }
            Op::Unary(op) => {
                let value = self.pop();
                let result = match (op, value) {
                    (UnaryOp::Neg, Value::Num(n)) => Value::Num(-n),
                    (UnaryOp::Neg, Value::Int(n)) => match n.checked_neg() {
                        Some(n) => Value::Int(n),
                        None => return Err(int_overflow(span)),
                    },
                    (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
                    (op, value) => {
                        return Err(RuntimeError::new(
                            span,
                            format!("{}に `{op}` は使えません", value.type_name()),
                        ));
                    }
                };
                self.stack.push(result);
            }
            Op::MakeArray(n) => {
                let items = self.split_top(*n as usize);
                self.stack.push(gc::new_array(items));
            }
            Op::ArrayNew => self.stack.push(gc::new_array(Vec::new())),
            Op::ArrayAppend => {
                let value = self.pop();
                if let Some(Value::Array(items)) = self.stack.last() {
                    items.borrow_mut().push(value);
                }
            }
            Op::ArrayExtend => match self.pop() {
                Value::Array(source) => {
                    if let Some(Value::Array(items)) = self.stack.last() {
                        let more = source.borrow().clone();
                        items.borrow_mut().extend(more);
                    }
                }
                other => {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "`...` で展開できるのは配列だけです（{}でした）",
                            other.type_name()
                        ),
                    ));
                }
            },
            Op::CheckKey => {
                let top = self.stack.last().expect("スタックに値がある");
                to_key(top, span)?;
            }
            Op::MakeMap(n) => {
                let pairs = self.split_top(*n as usize * 2);
                let mut map = Map::default();
                let mut it = pairs.into_iter();
                while let (Some(key), Some(value)) = (it.next(), it.next()) {
                    let Value::Str(key) = key else {
                        unreachable!("CheckKey で文字列だと確かめた");
                    };
                    map.insert(key, value);
                }
                self.stack.push(gc::new_map(map));
            }
            Op::ToDisplay => {
                let value = self.pop();
                self.stack.push(Value::Str(value.to_string().into()));
            }
            Op::Concat(n) => {
                let parts = self.split_top(*n as usize);
                let mut s = String::new();
                for part in parts {
                    if let Value::Str(text) = part {
                        s.push_str(&text);
                    }
                }
                self.stack.push(Value::Str(s.into()));
            }
            Op::IndexGet { target, key } => {
                let k = self.pop();
                let container = self.pop();
                self.stack.push(index_get(&container, &k, *target, *key)?);
            }
            Op::IndexSet { target, key } => {
                let value = self.pop();
                let k = self.pop();
                let container = self.pop();
                index_set(&container, &k, value, *target, *key)?;
            }
            Op::RejectModule => {
                let top = self.stack.last().expect("スタックに値がある");
                reject_module(top, span)?;
            }
            Op::Member { name, target } => {
                let base = self.pop();
                let name = &chunk.names[*name as usize];
                let value = self.member(base, name, *target, span)?;
                self.stack.push(value);
            }
            Op::FuncRef(n) => {
                let value = self.func_value(&chunk.names[*n as usize], span)?;
                self.stack.push(value);
            }
            Op::MakeClosure(i) => {
                let def = Rc::clone(&chunk.defs[*i as usize]);
                self.stack.push(Value::Func(Closure::register(Closure {
                    def,
                    env: Rc::clone(&self.env),
                    module: Rc::clone(&self.current),
                })));
            }
            Op::Try => match self.pop() {
                Value::Error(e) => {
                    let in_func = matches!(
                        self.vm_frames.last().map(|f| &f.kind),
                        Some(FrameKind::Function { .. })
                    );
                    if in_func {
                        // いまの関数から、そのエラー値を返す
                        return Ok(self.do_return(Value::Error(e), target));
                    }
                    // 関数の外では、エラーで止まる
                    return Err(RuntimeError::new(span, &*e.message));
                }
                value => self.stack.push(value),
            },
            Op::Return => {
                let value = self.pop();
                return Ok(self.do_return(value, target));
            }
            Op::Call { name, argc } => {
                let args = self.split_top(*argc as usize);
                return self.call_named(&chunk.names[*name as usize], args, span, *ip);
            }
            Op::CallSpread { name } => {
                let args = self.pop_spread_args();
                return self.call_named(&chunk.names[*name as usize], args, span, *ip);
            }
            Op::CallValue { argc, callee } => {
                let args = self.split_top(*argc as usize);
                let f = self.pop();
                return self.call_function_op(&f, args, span, *callee, *ip);
            }
            Op::CallValueSpread { callee } => {
                let args = self.pop_spread_args();
                let f = self.pop();
                return self.call_function_op(&f, args, span, *callee, *ip);
            }
            Op::MemberCall { name, argc, target } => {
                let args = self.split_top(*argc as usize);
                let base = self.pop();
                return self.member_call(
                    base,
                    &chunk.names[*name as usize],
                    args,
                    span,
                    *target,
                    *ip,
                );
            }
            Op::MemberCallSpread { name, target } => {
                let args = self.pop_spread_args();
                let base = self.pop();
                return self.member_call(
                    base,
                    &chunk.names[*name as usize],
                    args,
                    span,
                    *target,
                    *ip,
                );
            }
            Op::SkipIfGiven { index, target } => {
                let given = self.vm_frames.last().map_or(0, |f| f.given);
                if (*index as usize) < given {
                    *ip = *target as usize;
                }
            }
            Op::Destructure { pattern, declare } => {
                let value = self.pop();
                let pattern: &Pattern = &chunk.patterns[*pattern as usize];
                self.destructure(pattern, &value, *declare, span)?;
            }
            Op::ForPrep => match self.pop() {
                // 回し始めた時点の要素を使う
                Value::Array(items) => {
                    let snapshot = items.borrow().clone();
                    self.stack
                        .push(Value::Array(Rc::new(RefCell::new(snapshot))));
                    self.stack.push(Value::Num(0.0));
                }
                other => {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "for で回せるのは配列だけです（{}でした）。マップは `map_keys` でキーの配列にします",
                            other.type_name()
                        ),
                    ));
                }
            },
            Op::ForNext { end, var } => {
                let len = self.stack.len();
                let (Value::Num(position), Value::Array(items)) =
                    (&self.stack[len - 1], &self.stack[len - 2])
                else {
                    unreachable!("ForPrep が積んだ値");
                };
                let i = *position as usize;
                let item = items.borrow().get(i).cloned();
                match item {
                    None => *ip = *end as usize,
                    Some(item) => {
                        self.stack[len - 1] = Value::Num((i + 1) as f64);
                        gc::maybe_collect();
                        // ループ変数と本体は、繰り返しごとに新しい同じスコープ
                        let inner = Env::new(Some(Rc::clone(&self.env)));
                        inner.declare(&chunk.names[*var as usize], item);
                        let outer = mem::replace(&mut self.env, inner);
                        self.vm_frames
                            .last_mut()
                            .expect("枠がある")
                            .scopes
                            .push(outer);
                    }
                }
            }
        }
        Ok(Step::Next)
    }

    /// 展開を含む引数の配列を取り出す
    fn pop_spread_args(&mut self) -> Vec<Value> {
        match self.pop() {
            Value::Array(items) => items.borrow().clone(),
            _ => Vec::new(),
        }
    }

    /// `@u.name`: マップなら `@m{"name"}` と同じ。モジュールなら変数、なければ同じ名前の関数を関数値にする
    fn member(&mut self, base: Value, name: &str, target: Span, span: Span) -> RResult<Value> {
        if let Value::Map(_) = base {
            return index_get(&base, &Value::Str(name.into()), target, span);
        }
        let module = self.as_module(base, target)?;
        // 変数がなければ、同じ名前の関数を関数値として返す（design.md 5.8）
        let value = module.globals.get_own(name).or_else(|| {
            if let Some(native) = module.natives.get(name) {
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
        })
    }

    /// `name[args]`: ファイルの関数、`import`、組み込み関数の順に探して呼ぶ
    fn call_named(&mut self, name: &str, args: Vec<Value>, span: Span, ip: usize) -> RResult<Step> {
        let def = self.current.functions.borrow().get(name).cloned();
        if let Some(def) = def {
            let module = Rc::clone(&self.current);
            let env = Rc::clone(&module.globals);
            self.save_ip(ip);
            self.push_function_frame(&module, &def, &env, args, span)?;
            return Ok(Step::Reload);
        }
        let value = if name == "import" {
            self.import(&args, span)?
        } else {
            match self.native(name) {
                Some(f) => self.call_native(f, &args, span)?,
                None => {
                    return Err(RuntimeError::new(span, format!("未定義の関数 `{name}`")));
                }
            }
        };
        self.stack.push(value);
        Ok(Step::Next)
    }

    /// 関数値を呼ぶ命令。Dolphin の関数なら枠を積むだけ（戻りを待たない）
    fn call_function_op(
        &mut self,
        f: &Value,
        args: Vec<Value>,
        span: Span,
        callee_span: Span,
        ip: usize,
    ) -> RResult<Step> {
        match f {
            Value::Func(closure) => {
                self.save_ip(ip);
                self.push_function_frame(&closure.module, &closure.def, &closure.env, args, span)?;
                Ok(Step::Reload)
            }
            Value::Native(native) => {
                let value = self.call_native(native.f, &args, span)?;
                self.stack.push(value);
                Ok(Step::Next)
            }
            other => Err(not_callable(other, callee_span)),
        }
    }

    /// `@u.name[args]`: マップの値の関数、モジュールの関数を呼ぶ
    fn member_call(
        &mut self,
        base: Value,
        name: &str,
        args: Vec<Value>,
        span: Span,
        target: Span,
        ip: usize,
    ) -> RResult<Step> {
        let module = match base {
            // マップの値の関数を呼ぶ（`@m.handler[1]`）
            Value::Map(_) => {
                let f = index_get(&base, &Value::Str(name.into()), target, span)?;
                return self.call_function_op(&f, args, span, span, ip);
            }
            base => self.as_module(base, target)?,
        };
        let def = module.functions.borrow().get(name).cloned();
        match (def, module.natives.get(name)) {
            (Some(def), _) => {
                let env = Rc::clone(&module.globals);
                self.save_ip(ip);
                self.push_function_frame(&module, &def, &env, args, span)?;
                Ok(Step::Reload)
            }
            (None, Some(native)) => {
                let value = self.call_native(native.f, &args, span)?;
                self.stack.push(value);
                Ok(Step::Next)
            }
            (None, None) => Err(RuntimeError::new(
                span,
                format!("{} に関数 `{name}` がありません", module_label(&module)),
            )),
        }
    }
}

/// 二項演算（`&&` と `||` は短絡するので、ここには来ない）
fn binary_values(op: BinaryOp, l: &Value, r: &Value, span: Span) -> RResult<Value> {
    use BinaryOp as B;

    let value = match (op, l, r) {
        (B::Eq, _, _) => Value::Bool(l.equals(r)),
        (B::NotEq, _, _) => Value::Bool(!l.equals(r)),
        (B::Add, Value::Str(a), Value::Str(b)) => Value::Str(format!("{a}{b}").into()),
        (B::Div | B::Rem, Value::Num(_), Value::Num(b)) if *b == 0.0 => {
            return Err(RuntimeError::new(span, "0 で割ることはできません"));
        }
        (B::Div | B::Rem, Value::Int(_), Value::Int(0)) => {
            return Err(RuntimeError::new(span, "0 で割ることはできません"));
        }
        (_, Value::Int(a), Value::Int(b)) => match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                let result = match op {
                    B::Add => a.checked_add(*b),
                    B::Sub => a.checked_sub(*b),
                    B::Mul => a.checked_mul(*b),
                    B::Div => a.checked_div(*b),
                    _ => a.checked_rem(*b),
                };
                Value::Int(result.ok_or_else(|| int_overflow(span))?)
            }
            _ => Value::Bool(super::compare(op, Some(a.cmp(b)))),
        },
        (_, Value::Num(a), Value::Num(b)) => match op {
            B::Add => Value::Num(a + b),
            B::Sub => Value::Num(a - b),
            B::Mul => Value::Num(a * b),
            B::Div => Value::Num(a / b),
            B::Rem => Value::Num(a % b),
            _ => Value::Bool(super::compare(op, a.partial_cmp(b))),
        },
        (B::Lt | B::LtEq | B::Gt | B::GtEq, Value::Str(a), Value::Str(b)) => {
            Value::Bool(super::compare(op, Some(a.cmp(b))))
        }
        (_, Value::Int(_), Value::Num(_)) | (_, Value::Num(_), Value::Int(_)) => {
            return Err(RuntimeError::new(
                span,
                format!(
                    "整数と数値に `{op}` は使えません（`to_int` か `to_num` でそろえてください）"
                ),
            ));
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
