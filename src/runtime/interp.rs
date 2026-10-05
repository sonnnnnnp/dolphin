//! AST の木たどり評価器

use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use super::env::{Frame, Scope};
use super::error::RuntimeError;
use super::value::Value;
use crate::syntax::ast::{FuncDef, Program, StmtKind, Var};

/// 組み込み関数。エラー位置は呼び出し側で付けるので、メッセージだけ返す
pub type NativeFn = fn(&mut Interpreter<'_>, &[Value]) -> Result<Value, String>;

pub struct Interpreter<'a> {
    /// `log` の出力先。テストでは Vec<u8> を渡して出力を取り出す
    pub out: &'a mut dyn Write,
    globals: Scope,
    frames: Vec<Frame>,
    functions: HashMap<String, Rc<FuncDef>>,
    natives: HashMap<&'static str, NativeFn>,
}

impl<'a> Interpreter<'a> {
    pub fn new(out: &'a mut dyn Write) -> Self {
        Self {
            out,
            globals: Scope::default(),
            frames: Vec::new(),
            functions: HashMap::new(),
            natives: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &'static str, f: NativeFn) {
        self.natives.insert(name, f);
    }

    pub fn native(&self, name: &str) -> Option<NativeFn> {
        self.natives.get(name).copied()
    }

    pub fn get_var(&self, var: &Var) -> Option<&Value> {
        match var {
            Var::Global(name) => self.globals.get(name),
            Var::Local(name) => self.frames.last()?.locals.get(name),
        }
    }

    /// `$x` は関数の中でしか使えないことを Resolver が保証している前提
    pub fn set_var(&mut self, var: &Var, value: Value) {
        match var {
            Var::Global(name) => self.globals.set(name, value),
            Var::Local(name) => {
                let frame = self.frames.last_mut().expect("関数の外で $ 変数");
                frame.locals.set(name, value);
            }
        }
    }

    pub fn run(&mut self, program: &Program) -> Result<(), RuntimeError> {
        // 定義より前の行から呼べるように、先に全関数を登録する（design.md 5.2）
        for stmt in &program.stmts {
            if let StmtKind::FuncDef(def) = &stmt.kind {
                self.functions.insert(def.name.clone(), Rc::clone(def));
            }
        }
        todo!("段階 1: 文を順に評価する")
    }
}
