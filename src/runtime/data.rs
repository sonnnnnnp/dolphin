//! スレッドをまたげる値（design.md 5.23）。`Value` は `Rc` を持つので、補助スレッドで進めた
//! 処理の結果は、いったん `Data` にしてインタプリタのスレッドへ送り、そこで `Value` に戻す

use super::gc;
use super::map::Map;
use super::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Nil,
    Bool(bool),
    Num(f64),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Data>),
    Map(Vec<(String, Data)>),
    Error {
        kind: String,
        message: String,
        data: Option<Box<Data>>,
    },
}

impl Data {
    /// 値を `Data` にする。関数・モジュール・ハンドルなど、持ち出せないものは `Nil` になる
    pub fn from_value(value: &Value) -> Data {
        match value {
            Value::Bool(b) => Data::Bool(*b),
            Value::Num(n) => Data::Num(*n),
            Value::Int(n) => Data::Int(*n),
            Value::Str(s) => Data::Str(s.to_string()),
            Value::Bytes(b) => Data::Bytes(b.to_vec()),
            Value::Array(items) => {
                Data::Array(items.borrow().iter().map(Data::from_value).collect())
            }
            Value::Map(map) => Data::Map(
                map.borrow()
                    .iter()
                    .map(|(k, v)| (k.to_string(), Data::from_value(v)))
                    .collect(),
            ),
            Value::Error(e) => Data::Error {
                kind: e.kind.to_string(),
                message: e.message.to_string(),
                data: e.data.as_ref().map(|d| Box::new((**d).clone())),
            },
            _ => Data::Nil,
        }
    }

    pub fn into_value(self) -> Value {
        match self {
            Data::Nil => Value::Nil,
            Data::Bool(b) => Value::Bool(b),
            Data::Num(n) => Value::Num(n),
            Data::Int(n) => Value::Int(n),
            Data::Str(s) => Value::Str(s.into()),
            Data::Bytes(b) => Value::bytes(b),
            Data::Array(items) => gc::new_array(items.into_iter().map(Data::into_value).collect()),
            Data::Map(entries) => {
                let mut map = Map::default();
                for (key, value) in entries {
                    map.insert(key.as_str().into(), value.into_value());
                }
                gc::new_map(map)
            }
            Data::Error {
                kind,
                message,
                data,
            } => Value::error_with(&kind, message, data.map(|d| *d)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut map = Map::default();
        map.insert("a".into(), Value::Num(1.0));
        map.insert("i".into(), Value::Int(i64::MAX));
        map.insert(
            "b".into(),
            gc::new_array(vec![Value::Str("x".into()), Value::Nil]),
        );
        map.insert("c".into(), Value::bytes([1, 2]));
        map.insert("d".into(), Value::error("k", "m"));
        map.insert(
            "e".into(),
            Value::error_with(
                "k",
                "m",
                Some(Data::Map(vec![("n".into(), Data::Num(1.0))])),
            ),
        );
        let value = gc::new_map(map);
        let data = Data::from_value(&value);
        // スレッドをまたげる
        let moved = std::thread::spawn(move || data).join().unwrap();
        assert!(moved.clone().into_value().equals(&value));
        assert_eq!(
            Data::from_value(&moved.into_value()),
            Data::from_value(&value)
        );
    }
}
