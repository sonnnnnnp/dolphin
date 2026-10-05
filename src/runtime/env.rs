//! 変数の置き場所（design.md 5.1）

use std::collections::HashMap;

use super::value::Value;

/// 変数名 → 値。グローバル（`@x`）とローカル（`$x`）の両方に使う
#[derive(Debug, Default)]
pub struct Scope {
    vars: HashMap<String, Value>,
}

impl Scope {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// 最初の代入が宣言を兼ねる
    pub fn set(&mut self, name: &str, value: Value) {
        self.vars.insert(name.to_string(), value);
    }
}

/// 関数呼び出し 1 回分。実行時エラーの呼び出し履歴にも使う
#[derive(Debug)]
pub struct Frame {
    pub func_name: String,
    pub locals: Scope,
}
