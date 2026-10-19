//! AST の tree-walking 評価器

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::bytecode::Chunk;
use super::compile::compile_program;
use super::env::Env;
use super::error::{RuntimeError, TraceEntry};
use super::events::Events;
use super::gc;
use super::map::Map;
use super::module::Module;
use super::value::{Closure, NativeRef, Value};
use crate::diag::Origin;
use crate::sema::resolver::{self, Callee};
use crate::syntax::ast::{BinaryOp, FuncDef, Pattern, Program, Stmt, StmtKind, Var};
use crate::syntax::span::Span;
use crate::syntax::{lexer, parser};

mod vm;

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
    /// バイトコード VM の枠のスタック（design.md 5.45）
    vm_frames: Vec<vm::VmFrame>,
    /// VM の値のスタック
    stack: Vec<Value>,
    /// 関数の呼び出しの深さ（ファイルの本体は数えない）
    depth: usize,
    /// コンパイル済みの関数（定義のアドレス → 定義と命令列）
    chunks: HashMap<usize, (Rc<FuncDef>, Rc<Chunk>)>,
    /// 止まっている `async` 関数の流れ（番号 → 流れ。design.md 5.46）
    coroutines: HashMap<u64, vm::Coroutine>,
    next_coroutine: u64,
    /// 再開できる（待っていた future が終わった）流れの番号
    ready: std::collections::VecDeque<u64>,
    /// いま動かしている流れ
    current_coroutine: Option<u64>,
    natives: HashMap<&'static str, NativeFn>,
    /// いま実行している組み込み関数の呼び出し位置。組み込み関数が関数値を呼ぶときの位置になる
    native_span: Span,
    /// 組み込み関数が呼んだ関数値の中で起きたエラー。`call_value` が預け、
    /// 組み込み関数がエラーを返したときに `call_native` が取り出す（呼び出し履歴を保つため）
    native_error: Option<RuntimeError>,
    /// 組み込みモジュールの登録（`std:json` → 関数の表）と、import 済みのもの
    std_registry: HashMap<&'static str, Vec<(&'static str, NativeFn)>>,
    std_loaded: HashMap<&'static str, Rc<Module>>,
    /// 組み込みモジュールの定数（`std:math` の `pi` など）
    std_consts: HashMap<&'static str, Vec<(&'static str, Const)>>,
    /// イベントループ（design.md 5.23）
    pub events: Events,
    /// ワーカーの中で動いているときの、親とやり取りする口（design.md 5.43）
    pub worker: Option<super::worker::WorkerContext>,
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
            vm_frames: Vec::new(),
            stack: Vec::new(),
            depth: 0,
            chunks: HashMap::new(),
            coroutines: HashMap::new(),
            next_coroutine: 1,
            ready: std::collections::VecDeque::new(),
            current_coroutine: None,
            natives: HashMap::new(),
            native_span: Span { line: 1, col: 1 },
            native_error: None,
            std_registry: HashMap::new(),
            std_loaded: HashMap::new(),
            std_consts: HashMap::new(),
            events: Events::default(),
            worker: None,
        }
    }

    /// いま実行しているファイルのあるディレクトリ（`import` の相対パスの基準）
    pub fn current_dir(&self) -> &Path {
        &self.current.dir
    }

    /// 再開を待つ流れがあるか（スクリプトの最後で、イベントループを動かす必要があるか）
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    /// 組み込み関数の名前（並べ替え済み。言語サーバーの補完用）
    pub fn native_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.natives.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// 組み込みモジュールと、そのメンバー（関数と定数）の名前。どちらも並べ替え済み
    pub fn std_modules(&self) -> Vec<(&'static str, Vec<&'static str>)> {
        let mut modules: Vec<_> = self
            .std_registry
            .iter()
            .map(|(&name, functions)| {
                let mut members: Vec<&'static str> = functions
                    .iter()
                    .map(|&(full, _)| full.rsplit('.').next().unwrap_or(full))
                    .chain(
                        self.std_consts
                            .get(name)
                            .into_iter()
                            .flatten()
                            .map(|&(n, _)| n),
                    )
                    .collect();
                members.sort_unstable();
                (name, members)
            })
            .collect();
        modules.sort_unstable();
        modules
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
            .map(|def| (def.min_args(), def.max_args()));
        match arity {
            Some((min, max)) => Some(Callee::User { min, max }),
            None => self.native_callee(name),
        }
    }

    /// 組み込み関数か。`import` は評価器が直接扱うが、Resolver からは組み込み関数に見える
    fn native_callee(&self, name: &str) -> Option<Callee> {
        (name == "import" || self.natives.contains_key(name)).then_some(Callee::Native)
    }

    /// 実行中のファイルの名前つきの関数を、定義の順に（名前、引数の数）で返す
    pub fn user_functions(&self) -> Vec<(String, usize)> {
        let mut defs: Vec<_> = self
            .current
            .functions
            .borrow()
            .values()
            .map(|def| (def.span, def.name.clone(), def.params.len()))
            .collect();
        defs.sort_by_key(|(span, _, _)| (span.line, span.col));
        defs.into_iter().map(|(_, name, n)| (name, n)).collect()
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
        let returns_value = keep_last
            && matches!(
                program.stmts.last(),
                Some(Stmt {
                    kind: StmtKind::Expr(_),
                    ..
                })
            );
        let chunk = compile_program(&program.stmts, keep_last);
        let target = self.vm_frames.len();
        self.push_program_frame(chunk);
        let value = self.run_vm(target)?;
        Ok(returns_value.then_some(value))
    }

    /// 分割代入。`pattern` の変数に、`value` の要素を宣言か代入で入れる
    fn destructure(
        &mut self,
        pattern: &Pattern,
        value: &Value,
        declare: bool,
        span: Span,
    ) -> RResult<()> {
        match pattern {
            Pattern::Var(var) => {
                if declare {
                    self.env.declare(&var.0, value.clone());
                } else if !self.env.assign(&var.0, value.clone()) {
                    return Err(undefined(var, span));
                }
            }
            Pattern::Array { items, rest } => {
                let Value::Array(array) = value else {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "配列のパターンの右辺は配列である必要があります（{}でした）",
                            value.type_name()
                        ),
                    ));
                };
                let elements = array.borrow().clone();
                if elements.len() < items.len() {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "分割代入の要素が足りません（{} 個必要、配列は {} 個）",
                            items.len(),
                            elements.len()
                        ),
                    ));
                }
                for (item, element) in items.iter().zip(&elements) {
                    self.destructure(item, element, declare, span)?;
                }
                if let Some(rest) = rest {
                    let remaining = gc::new_array(elements[items.len()..].to_vec());
                    self.destructure(&Pattern::Var(rest.clone()), &remaining, declare, span)?;
                }
            }
            Pattern::Map(entries) => {
                let Value::Map(map) = value else {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "マップのパターンの右辺はマップである必要があります（{}でした）",
                            value.type_name()
                        ),
                    ));
                };
                for (key, entry) in entries {
                    let element = map.borrow().get(key).cloned();
                    let Some(element) = element else {
                        return Err(RuntimeError::new(
                            span,
                            format!("キー {key:?} がありません"),
                        ));
                    };
                    self.destructure(entry, &element, declare, span)?;
                }
            }
        }
        Ok(())
    }

    // ---- 関数呼び出し ----

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

    /// 関数値を呼んで、戻り値を返す。`span` は呼び出し位置、`callee_span` は呼ぶ式の位置
    /// （組み込み関数から呼ぶとき用。入れ子で VM を動かして、戻るまで待つ）
    fn call_function(
        &mut self,
        f: &Value,
        args: Vec<Value>,
        span: Span,
        callee_span: Span,
    ) -> RResult<Value> {
        match f {
            Value::Func(closure) if closure.def.is_async => {
                self.start_coroutine(&closure.module, &closure.def, &closure.env, args, span)
            }
            Value::Func(closure) => {
                let target = self.vm_frames.len();
                self.push_function_frame(&closure.module, &closure.def, &closure.env, args, span)?;
                self.run_vm(target)
            }
            Value::Native(native) => self.call_native(native.f, &args, span),
            other => Err(not_callable(other, callee_span)),
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

    fn as_module(&self, value: Value, span: Span) -> RResult<Rc<Module>> {
        match value {
            Value::Module(module) => Ok(module),
            other => Err(RuntimeError::new(
                span,
                format!(
                    "`.` を使えるのはモジュールかマップだけです（{}でした）",
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
        let full = super::module::resolve_import(&self.current.dir, rel)
            .map_err(|e| RuntimeError::new(span, e.to_string()))?;
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
        let result = self.run(&program, false);
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
                "std:regex" if !cfg!(feature = "regex") => Some("regex"),
                "std:ui" if !cfg!(feature = "ui") => Some("ui"),
                "std:audio" if !cfg!(feature = "audio") => Some("audio"),
                "std:sqlite" if !cfg!(feature = "sqlite") => Some("sqlite"),
                "std:ffi" if !cfg!(feature = "ffi") => Some("ffi"),
                "std:image" if !cfg!(feature = "image") => Some("image"),
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
        // Dolphin で書いた関数（`async.all` など）
        if key == "std:async" {
            let tokens = lexer::tokenize(crate::stdlib::async_::PRELUDE)
                .expect("std:async の Dolphin のコードは字句に分けられる");
            let program =
                parser::parse(&tokens).expect("std:async の Dolphin のコードは解析できる");
            for stmt in &program.stmts {
                if let StmtKind::FuncDef(def) = &stmt.kind {
                    module
                        .functions
                        .borrow_mut()
                        .insert(def.name.clone(), Rc::clone(def));
                }
            }
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

fn undefined_name(name: &str, span: Span) -> RuntimeError {
    RuntimeError::new(span, format!("未定義の変数 `@{name}`"))
}

/// 関数でない値を `[ ]` で呼ぼうとした
fn not_callable(value: &Value, callee_span: Span) -> RuntimeError {
    let hint = match value {
        Value::Array(_) | Value::Map(_) => {
            "。配列やマップの要素は `@a{0}` のように `{ }` で読みます"
        }
        _ => "",
    };
    RuntimeError::new(
        callee_span,
        format!(
            "`[ ]` で呼び出せるのは関数だけです（{}でした）{hint}",
            value.type_name()
        ),
    )
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

/// モジュールの変数は、外から書き換えられない（`@u.x = 1`）
fn reject_module(container: &Value, span: Span) -> RResult<()> {
    match container {
        Value::Module(_) => Err(RuntimeError::new(
            span,
            "モジュールの変数は外から書き換えられません（書き換えたいときは、そのファイルに setter 関数を書いてください）",
        )),
        _ => Ok(()),
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
    let n = match *value {
        Value::Num(n) => n,
        Value::Int(i) => {
            return usize::try_from(i).ok().filter(|i| *i < len).ok_or_else(|| {
                RuntimeError::new(span, format!("添字 {i} は範囲外です（要素は {len} 個）"))
            });
        }
        _ => {
            return Err(RuntimeError::new(
                span,
                format!(
                    "添字は数値か整数である必要があります（{}でした）",
                    value.type_name()
                ),
            ));
        }
    };
    if n.fract() != 0.0 || n < 0.0 || n >= len as f64 {
        return Err(RuntimeError::new(
            span,
            format!("添字 {n} は範囲外です（要素は {len} 個）"),
        ));
    }
    Ok(n as usize)
}

fn int_overflow(span: Span) -> RuntimeError {
    RuntimeError::new(span, "整数が範囲（-2^63 〜 2^63-1）を超えました")
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
