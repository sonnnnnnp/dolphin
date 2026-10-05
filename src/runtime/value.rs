//! 値の型（design.md 5.3）

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::map::Map;

#[derive(Debug, Clone)]
pub enum Value {
    /// 値なし（戻り値のない関数の結果）
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    /// 配列とマップは参照渡し（design.md 7.1）。clone しても同じものを指す
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<Map>>),
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
            Value::Map(_) => "マップ",
        }
    }

    /// `==` の結果。型が違えば false。配列は要素を順に、マップは順序を見ずに比べる
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Map(a), Value::Map(b)) => {
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.equals(w)))
            }
            _ => false,
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
                    write!(f, "{}", Nested(item))?;
                }
                write!(f, "}}")
            }
            Value::Map(map) => {
                let map = map.borrow();
                if map.is_empty() {
                    return write!(f, "{{:}}");
                }
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{key:?}: {}", Nested(value))?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// 配列やマップの中での表示。文字列だけ `"` で囲む（`{"a", "b"}` と `{a, b}` を区別するため）
struct Nested<'a>(&'a Value);

impl fmt::Display for Nested<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Str(s) => write!(f, "{s:?}"),
            other => write!(f, "{other}"),
        }
    }
}
