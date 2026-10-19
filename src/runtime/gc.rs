//! 循環回収器（design.md 5.15）
//!
//! 値は `Rc` で持っているので、循環した参照は解放されない。配列・マップ・スコープ・関数値を
//! 作るときに弱参照で登録しておき、参照カウントだけを使って、外から届かない循環を見つけて壊す
//! （Bacon–Rajan 型の試行削除）。ルートの一覧は要らない。Rust のスタックなどから持たれている
//! 対象は、参照カウントが登録簿の中の参照より多いので、自然に「生きている」と判定される

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem;
use std::rc::{Rc, Weak};

use super::env::Env;
use super::map::Map;
use super::value::{Closure, Value};

/// 前回の回収から作った対象がこの数に達するまでは、自動では回収しない
const MIN_THRESHOLD: usize = 10_000;

enum Entry {
    Array(Weak<RefCell<Vec<Value>>>),
    Map(Weak<RefCell<Map>>),
    Env(Weak<Env>),
    Closure(Weak<Closure>),
}

enum Node {
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<Map>>),
    Env(Rc<Env>),
    Closure(Rc<Closure>),
}

#[derive(Default)]
struct Registry {
    entries: RefCell<Vec<Entry>>,
    /// 前回の回収から登録した数
    created: Cell<usize>,
    /// 前回の回収のあとに生き残っていた数
    live: Cell<usize>,
}

thread_local! {
    static REGISTRY: Registry = Registry::default();
}

fn register(entry: Entry) {
    REGISTRY.with(|r| {
        r.entries.borrow_mut().push(entry);
        r.created.set(r.created.get() + 1);
    });
}

pub fn new_array(items: Vec<Value>) -> Value {
    let rc = Rc::new(RefCell::new(items));
    register(Entry::Array(Rc::downgrade(&rc)));
    Value::Array(rc)
}

pub fn new_map(map: Map) -> Value {
    let rc = Rc::new(RefCell::new(map));
    register(Entry::Map(Rc::downgrade(&rc)));
    Value::Map(rc)
}

pub fn register_env(env: &Rc<Env>) {
    register(Entry::Env(Rc::downgrade(env)));
}

pub fn register_closure(closure: &Rc<Closure>) {
    register(Entry::Closure(Rc::downgrade(closure)));
}

/// 前回の回収から十分な数を作っていれば回収する。実行中のどこで呼んでも安全
pub fn maybe_collect() {
    let due = REGISTRY.with(|r| r.created.get() >= MIN_THRESHOLD.max(r.live.get()));
    if due {
        collect();
    }
}

/// 回収する。回収した対象（配列・マップ・スコープ・関数値）の数を返す
pub fn collect() -> usize {
    // 回収中に登録簿を借りたまま解放が起きても困らないよう、弱参照は先に取り出す
    let entries = REGISTRY.with(|r| mem::take(&mut *r.entries.borrow_mut()));
    let mut nodes: Vec<Node> = Vec::with_capacity(entries.len());
    let mut alive: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        let node = match &entry {
            Entry::Array(w) => w.upgrade().map(Node::Array),
            Entry::Map(w) => w.upgrade().map(Node::Map),
            Entry::Env(w) => w.upgrade().map(Node::Env),
            Entry::Closure(w) => w.upgrade().map(Node::Closure),
        };
        if let Some(node) = node {
            nodes.push(node);
            alive.push(entry);
        }
    }

    let index: HashMap<usize, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.addr(), i))
        .collect();

    // 外から持たれている参照の数 = 全体の参照数 - 取り出すために増やした 1 - 登録簿の対象どうしの参照
    let mut external: Vec<i64> = nodes.iter().map(|n| n.strong_count() as i64 - 1).collect();
    let mut children: Vec<Vec<usize>> = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let mut kids = Vec::new();
        // 借りられない（借用中の）対象は、中を数えない。子の参照が外からと見なされ、生きている扱いになる
        node.for_each_child(&mut |addr| {
            if let Some(&j) = index.get(&addr) {
                external[j] -= 1;
                kids.push(j);
            }
        });
        children.push(kids);
    }

    // 外から持たれている対象と、そこから辿れる対象が生きている
    let mut marked = vec![false; nodes.len()];
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| external[i] > 0).collect();
    for &i in &stack {
        marked[i] = true;
    }
    while let Some(i) = stack.pop() {
        for &j in &children[i] {
            if !marked[j] {
                marked[j] = true;
                stack.push(j);
            }
        }
    }

    // ごみの中身を空にして、循環を壊す。解放は nodes を落とすときに起きる
    let mut freed = 0;
    for (node, &live) in nodes.iter().zip(&marked) {
        if !live {
            freed += 1;
            node.clear();
        }
    }
    drop(nodes);
    drop(children);

    REGISTRY.with(|r| {
        // 解放されたものを除いて、登録簿を作り直す（回収中に作られた対象は残す）
        let mut entries = r.entries.borrow_mut();
        let created_meanwhile = mem::take(&mut *entries);
        alive.retain(|e| e.is_alive());
        alive.extend(created_meanwhile);
        r.live.set(alive.len());
        *entries = alive;
        r.created.set(0);
    });
    freed
}

impl Entry {
    fn is_alive(&self) -> bool {
        match self {
            Entry::Array(w) => w.strong_count() > 0,
            Entry::Map(w) => w.strong_count() > 0,
            Entry::Env(w) => w.strong_count() > 0,
            Entry::Closure(w) => w.strong_count() > 0,
        }
    }
}

impl Node {
    fn addr(&self) -> usize {
        match self {
            Node::Array(rc) => Rc::as_ptr(rc) as *const () as usize,
            Node::Map(rc) => Rc::as_ptr(rc) as *const () as usize,
            Node::Env(rc) => Rc::as_ptr(rc) as *const () as usize,
            Node::Closure(rc) => Rc::as_ptr(rc) as *const () as usize,
        }
    }

    fn strong_count(&self) -> usize {
        match self {
            Node::Array(rc) => Rc::strong_count(rc),
            Node::Map(rc) => Rc::strong_count(rc),
            Node::Env(rc) => Rc::strong_count(rc),
            Node::Closure(rc) => Rc::strong_count(rc),
        }
    }

    /// 直接持っている対象（配列・マップ・スコープ・関数値）のアドレスを順に渡す
    fn for_each_child(&self, f: &mut dyn FnMut(usize)) {
        match self {
            Node::Array(rc) => {
                if let Ok(items) = rc.try_borrow() {
                    items.iter().for_each(|v| value_child(v, f));
                }
            }
            Node::Map(rc) => {
                if let Ok(map) = rc.try_borrow() {
                    map.iter().for_each(|(_, v)| value_child(v, f));
                }
            }
            Node::Env(env) => env.for_each_child(f),
            Node::Closure(closure) => f(Rc::as_ptr(&closure.env) as *const () as usize),
        }
    }

    /// 中身を空にする。ごみだけに使う（外から持たれていないので、借用中ではない）
    fn clear(&self) {
        match self {
            Node::Array(rc) => {
                let items = mem::take(&mut *rc.borrow_mut());
                drop(items);
            }
            Node::Map(rc) => {
                let map = mem::take(&mut *rc.borrow_mut());
                drop(map);
            }
            Node::Env(env) => env.clear(),
            // 関数値は変更できない。循環は必ず配列・マップ・スコープのどれかを通る
            Node::Closure(_) => {}
        }
    }
}

/// 値が配列・マップ・関数値なら、そのアドレスを渡す
pub fn value_child(value: &Value, f: &mut dyn FnMut(usize)) {
    match value {
        Value::Array(rc) => f(Rc::as_ptr(rc) as *const () as usize),
        Value::Map(rc) => f(Rc::as_ptr(rc) as *const () as usize),
        Value::Func(rc) => f(Rc::as_ptr(rc) as *const () as usize),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array_of(value: Value) -> Rc<RefCell<Vec<Value>>> {
        match new_array(vec![value]) {
            Value::Array(rc) => rc,
            _ => unreachable!(),
        }
    }

    #[test]
    fn frees_unreachable_cycle() {
        let a = array_of(Value::Nil);
        a.borrow_mut()[0] = Value::Array(Rc::clone(&a));
        let weak = Rc::downgrade(&a);
        drop(a);
        // 循環のせいで、参照カウントだけでは解放されない
        assert!(weak.upgrade().is_some());
        assert!(collect() >= 1);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn keeps_reachable_cycle() {
        let a = array_of(Value::Nil);
        a.borrow_mut()[0] = Value::Array(Rc::clone(&a));
        collect();
        // 外（この変数）から持たれているので、中身も残る
        assert!(matches!(a.borrow()[0], Value::Array(_)));
    }

    #[test]
    fn keeps_what_a_live_object_points_to() {
        let inner = array_of(Value::Num(7.0));
        let outer = array_of(Value::Array(Rc::clone(&inner)));
        inner.borrow_mut().push(Value::Array(Rc::clone(&outer)));
        drop(inner);
        collect();
        let kept = match &outer.borrow()[0] {
            Value::Array(rc) => rc.borrow().len(),
            _ => 0,
        };
        assert_eq!(kept, 2);
    }

    #[test]
    fn frees_closure_scope_cycle() {
        use crate::runtime::module::Module;
        use crate::syntax::ast::FuncDef;
        use crate::syntax::span::Span;
        let module = Rc::new(Module::new("".into(), Default::default(), None));
        let env = Env::new(None);
        let def = Rc::new(FuncDef {
            name: String::new(),
            is_async: false,
            params: Vec::new(),
            rest: None,
            body: Vec::new(),
            span: Span { line: 1, col: 1 },
        });
        let f = Closure::register(Closure {
            def,
            env: Rc::clone(&env),
            module,
        });
        env.declare("f", Value::Func(f));
        let weak = Rc::downgrade(&env);
        drop(env);
        assert!(weak.upgrade().is_some());
        collect();
        assert!(weak.upgrade().is_none());
    }
}
