//! 値の型（design.md 5.3）

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    /// 値なし（戻り値のない関数の結果）
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    /// 7.1（配列の渡し方）が未決。いまは参照共有。
    /// 値渡しにするなら Rc<Vec<Value>> + Rc::make_mut に変える
    Array(Rc<RefCell<Vec<Value>>>),
}

impl Value {
    /// エラーメッセージ用の型名
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "値なし",
            Value::Bool(_) => "真偽値",
            Value::Num(_) => "数値",
            Value::Str(_) => "文字列",
            Value::Array(_) => "配列",
        }
    }
}

/// `log` や文字列展開での表示
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s}"),
            Value::Array(items) => {
                write!(f, "{{")?;
                for (i, item) in items.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "}}")
            }
        }
    }
}
