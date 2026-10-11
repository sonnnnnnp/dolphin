//! 値の型（design.md 5.3）

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::env::Env;
use super::interp::NativeFn;
use super::map::Map;
use super::module::Module;
use crate::syntax::ast::FuncDef;

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
    /// `import` の結果。同じものを指す（design.md 5.7）
    Module(Rc<Module>),
    /// 関数値（design.md 5.8）。定義と、作られた場所のスコープの組
    Func(Rc<Closure>),
    /// 関数値になった組み込み関数
    Native(NativeRef),
}

/// 関数値の実体。`env` は関数が作られた場所のスコープで、呼び出すたびにその内側に
/// 新しいスコープを作る。名前つきの関数は、そのファイルのトップレベルのスコープを持つ
pub struct Closure {
    pub def: Rc<FuncDef>,
    pub env: Rc<Env>,
    pub module: Rc<Module>,
}

/// `env` が自分自身を指しうるので、名前だけ出す
impl fmt::Debug for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Closure({:?})", self.def.name)
    }
}

#[derive(Clone)]
pub struct NativeRef {
    pub name: &'static str,
    pub f: NativeFn,
}

impl fmt::Debug for NativeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Native({:?})", self.name)
    }
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
            Value::Module(_) => "モジュール",
            Value::Func(_) | Value::Native(_) => "関数",
        }
    }

    /// `==` の結果。型が違えば false。配列は要素を順に、マップは順序を見ずに比べる
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            // 同じものなら中身を見ずに true（自分自身を含む配列で無限に比べないため）
            (Value::Array(a), Value::Array(b)) if Rc::ptr_eq(a, b) => true,
            (Value::Map(a), Value::Map(b)) if Rc::ptr_eq(a, b) => true,
            (Value::Module(a), Value::Module(b)) => Rc::ptr_eq(a, b),
            // 同じ定義を同じスコープから取り出したものは等しい（`add == add`）
            (Value::Func(a), Value::Func(b)) => {
                Rc::ptr_eq(a, b) || (Rc::ptr_eq(&a.def, &b.def) && Rc::ptr_eq(&a.env, &b.env))
            }
            (Value::Native(a), Value::Native(b)) => a.name == b.name,
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

/// `log` や文字列展開での表示。文字列はそのまま出す
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_value(f, self, false, &mut Vec::new())
    }
}

/// 文字列も `"` で囲む表示（REPL で式の値を見せるときに使う）
pub struct Repr<'a>(pub &'a Value);

impl fmt::Display for Repr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_value(f, self.0, true, &mut Vec::new())
    }
}

/// `seen` は今たどっている途中の配列・マップ。
/// 自分自身を含む配列（`@a[0] = @a`）で無限に表示し続けないよう、
/// 2 回目に出会ったら `{...}` と表示する（Python の `[...]` と同じ）
fn write_value(
    f: &mut fmt::Formatter<'_>,
    value: &Value,
    quote_str: bool,
    seen: &mut Vec<*const ()>,
) -> fmt::Result {
    let ptr = match value {
        Value::Nil => return write!(f, "nil"),
        Value::Bool(b) => return write!(f, "{b}"),
        Value::Num(n) => return write!(f, "{n}"),
        Value::Str(s) if quote_str => return write!(f, "{s:?}"),
        Value::Str(s) => return write!(f, "{s}"),
        Value::Module(m) => return write!(f, "<module {:?}>", &*m.name),
        Value::Func(c) if c.def.name.is_empty() => return write!(f, "<関数>"),
        Value::Func(c) => return write!(f, "<関数 {}>", c.def.name),
        Value::Native(n) => return write!(f, "<組み込み関数 {}>", n.name),
        Value::Array(items) => Rc::as_ptr(items).cast::<()>(),
        Value::Map(map) => Rc::as_ptr(map).cast::<()>(),
    };
    if seen.contains(&ptr) {
        return write!(f, "{{...}}");
    }

    seen.push(ptr);
    // 配列やマップの中の文字列は `"` で囲む（`{"a", "b"}` と `{a, b}` を区別するため）
    let result = match value {
        Value::Array(items) => {
            write!(f, "{{")?;
            for (i, item) in items.borrow().iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_value(f, item, true, seen)?;
            }
            write!(f, "}}")
        }
        Value::Map(map) => {
            let map = map.borrow();
            if map.is_empty() {
                write!(f, "{{:}}")
            } else {
                write!(f, "{{")?;
                for (i, (key, item)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{key:?}: ")?;
                    write_value(f, item, true, seen)?;
                }
                write!(f, "}}")
            }
        }
        _ => unreachable!("配列とマップ以外は先に return している"),
    };
    seen.pop();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_referencing_array() {
        let array = Rc::new(RefCell::new(vec![Value::Num(1.0)]));
        array.borrow_mut().push(Value::Array(Rc::clone(&array)));
        let value = Value::Array(array);
        assert_eq!(value.to_string(), "{1, {...}}");
        assert!(value.equals(&value.clone()));
    }

    #[test]
    fn same_array_twice_is_not_a_cycle() {
        let inner = Value::Array(Rc::new(RefCell::new(vec![Value::Num(1.0)])));
        let outer = Value::Array(Rc::new(RefCell::new(vec![inner.clone(), inner])));
        assert_eq!(outer.to_string(), "{{1}, {1}}");
    }

    #[test]
    fn function_display_and_equality() {
        use crate::syntax::ast::FuncDef;
        use crate::syntax::span::Span;

        let module = Rc::new(Module::new("".into(), Default::default(), None));
        let def = |name: &str| {
            Rc::new(FuncDef {
                name: name.into(),
                params: Vec::new(),
                body: Vec::new(),
                span: Span { line: 1, col: 1 },
            })
        };
        let make = |def: &Rc<FuncDef>| {
            Value::Func(Rc::new(Closure {
                def: Rc::clone(def),
                env: Rc::clone(&module.globals),
                module: Rc::clone(&module),
            }))
        };
        let (add, anon) = (def("add"), def(""));
        assert_eq!(make(&add).to_string(), "<関数 add>");
        assert_eq!(make(&anon).to_string(), "<関数>");
        // 同じ定義を同じスコープから取り出せば等しい。定義が違えば別
        assert!(make(&add).equals(&make(&add)));
        assert!(!make(&add).equals(&make(&anon)));
        assert_eq!(make(&add).type_name(), "関数");
    }

    #[test]
    fn repr_quotes_strings() {
        let s = Value::Str("a\"b".into());
        assert_eq!(s.to_string(), "a\"b");
        assert_eq!(Repr(&s).to_string(), r#""a\"b""#);
    }
}
