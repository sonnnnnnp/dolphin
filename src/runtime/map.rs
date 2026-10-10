//! マップ（design.md 5.4）。キーは文字列のみで、追加した順を保つ

use std::collections::HashMap;
use std::rc::Rc;

use super::value::Value;

/// 順序つきのマップ。`entries` が追加順の本体で、`index` はキーから位置を引くための索引
#[derive(Debug, Clone, Default)]
pub struct Map {
    entries: Vec<(Rc<str>, Value)>,
    index: HashMap<Rc<str>, usize>,
}

impl Map {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// キーがあれば値を上書きし（位置は変えない）、なければ末尾に追加する
    pub fn insert(&mut self, key: Rc<str>, value: Value) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(Rc::clone(&key), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    /// 取り除いた値を返す。後ろの要素は 1 つずつ前に詰める
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.index.remove(key)?;
        let (_, value) = self.entries.remove(i);
        for pos in self.index.values_mut() {
            if *pos > i {
                *pos -= 1;
            }
        }
        Some(value)
    }

    /// 追加順に (キー, 値) を返す
    pub fn iter(&self) -> impl Iterator<Item = (&Rc<str>, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(map: &Map) -> Vec<&str> {
        map.iter().map(|(k, _)| &**k).collect()
    }

    #[test]
    fn keeps_insertion_order() {
        let mut map = Map::default();
        map.insert("b".into(), Value::Num(1.0));
        map.insert("a".into(), Value::Num(2.0));
        map.insert("b".into(), Value::Num(3.0));
        assert_eq!(keys(&map), ["b", "a"]);
        assert!(matches!(map.get("b"), Some(Value::Num(3.0))));
    }

    #[test]
    fn remove_shifts_index() {
        let mut map = Map::default();
        for key in ["a", "b", "c"] {
            map.insert(key.into(), Value::Nil);
        }
        assert!(map.remove("a").is_some());
        assert!(map.remove("x").is_none());
        assert_eq!(keys(&map), ["b", "c"]);
        assert!(map.contains_key("c"));
        map.insert("c".into(), Value::Bool(true));
        assert!(matches!(map.get("c"), Some(Value::Bool(true))));
        assert_eq!(map.len(), 2);
    }
}
