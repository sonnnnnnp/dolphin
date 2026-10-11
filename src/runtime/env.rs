//! 変数の置き場所（design.md 5.1）

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::value::Value;

/// スコープ 1 つ分の変数表。外側のスコープへの参照を持ち、名前は内側から順に探す。
/// トップレベルのスコープ（ファイルの変数）は `parent` を持たない
#[derive(Debug, Default)]
pub struct Env {
    vars: RefCell<HashMap<String, Value>>,
    parent: Option<Rc<Env>>,
}

impl Env {
    pub fn new(parent: Option<Rc<Env>>) -> Rc<Self> {
        Rc::new(Self {
            vars: RefCell::default(),
            parent,
        })
    }

    /// 内側から順に探す
    pub fn get(&self, name: &str) -> Option<Value> {
        match self.vars.borrow().get(name) {
            Some(value) => Some(value.clone()),
            None => self.parent.as_ref()?.get(name),
        }
    }

    /// このスコープだけを探す（`@u.name` でモジュールの変数を読むときに使う）
    pub fn get_own(&self, name: &str) -> Option<Value> {
        self.vars.borrow().get(name).cloned()
    }

    /// このスコープに新しい変数を作る。同じ名前があれば置き換える
    /// （同じスコープの二重宣言は Resolver が弾く。REPL の再入力は置き換えを許す）
    pub fn declare(&self, name: &str, value: Value) {
        self.vars.borrow_mut().insert(name.to_string(), value);
    }

    /// 宣言済みの最も内側の変数を書き換える。見つからなければ false
    pub fn assign(&self, name: &str, value: Value) -> bool {
        if let Some(slot) = self.vars.borrow_mut().get_mut(name) {
            *slot = value;
            return true;
        }
        match &self.parent {
            Some(parent) => parent.assign(name, value),
            None => false,
        }
    }
}

/// 関数呼び出し 1 回分。呼び出しの深さを数えるために使う
#[derive(Debug)]
pub struct Frame {
    pub func_name: String,
}
