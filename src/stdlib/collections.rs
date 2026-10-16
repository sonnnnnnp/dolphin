//! `std:collections`: heap_push, heap_pop, heap_peek, heapify, counts, unique, group_by,
//! chunk, zip, flatten（design.md 5.32）。標準ライブラリだけで書く

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use super::{array_arg, expect_args, func_arg, int_arg, new_array, new_map};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:collections",
        &[
            ("collections.heap_push", heap_push),
            ("collections.heap_pop", heap_pop),
            ("collections.heap_peek", heap_peek),
            ("collections.heapify", heapify),
            ("collections.counts", counts),
            ("collections.unique", unique),
            ("collections.group_by", group_by),
            ("collections.chunk", chunk),
            ("collections.zip", zip),
            ("collections.flatten", flatten),
        ],
    );
}

fn count_range(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        return Ok(());
    }
    Err(format!(
        "{name} の引数は {min} 個から {max} 個です（{} 個渡されました）",
        args.len()
    ))
}

// ---- ヒープ ----
//
// 配列を二分ヒープとして使う。`cmp` はユーザーの関数で、その中でこの配列を触られても
// RefCell が壊れないよう、比べるたびに短く借りて、借りたまま関数を呼ばない

type Items = Rc<RefCell<Vec<Value>>>;

/// ヒープの順序: `cmp` があればそれ、なければ数値同士・文字列同士の昇順
struct Order<'a> {
    name: &'a str,
    cmp: Option<&'a Value>,
}

impl Order<'_> {
    /// `a` が `b` より先か（厳密に小さいか）
    fn less(&self, interp: &mut Interpreter<'_>, a: &Value, b: &Value) -> Result<bool, String> {
        if let Some(f) = self.cmp {
            return match interp.call_value(f, vec![a.clone(), b.clone()])? {
                Value::Num(n) if n.is_nan() => {
                    Err(format!("{} の関数が NaN を返しました", self.name))
                }
                Value::Num(n) => Ok(n < 0.0),
                other => Err(format!(
                    "{} の関数は数値を返す必要があります（{}でした）",
                    self.name,
                    other.type_name()
                )),
            };
        }
        match (a, b) {
            (Value::Num(a), Value::Num(b)) => Ok(a.total_cmp(b) == Ordering::Less),
            (Value::Str(a), Value::Str(b)) => Ok(a < b),
            _ => Err(format!(
                "{} の要素は、すべて数値かすべて文字列である必要があります（{}と{}を比べました。比較の関数を渡してください）",
                self.name,
                a.type_name(),
                b.type_name()
            )),
        }
    }
}

fn modified(name: &str) -> String {
    format!("{name} の途中でヒープが書き換えられました（比較の関数の中で触らないでください）")
}

fn item(name: &str, items: &Items, i: usize) -> Result<Value, String> {
    items.borrow().get(i).cloned().ok_or_else(|| modified(name))
}

fn swap(name: &str, items: &Items, i: usize, j: usize) -> Result<(), String> {
    let mut items = items.borrow_mut();
    if i >= items.len() || j >= items.len() {
        return Err(modified(name));
    }
    items.swap(i, j);
    Ok(())
}

fn sift_up(
    interp: &mut Interpreter<'_>,
    order: &Order<'_>,
    items: &Items,
    mut i: usize,
) -> Result<(), String> {
    while i > 0 {
        let parent = (i - 1) / 2;
        let (a, b) = (
            item(order.name, items, i)?,
            item(order.name, items, parent)?,
        );
        if !order.less(interp, &a, &b)? {
            break;
        }
        swap(order.name, items, i, parent)?;
        i = parent;
    }
    Ok(())
}

fn sift_down(
    interp: &mut Interpreter<'_>,
    order: &Order<'_>,
    items: &Items,
    mut i: usize,
) -> Result<(), String> {
    loop {
        let len = items.borrow().len();
        let mut smallest = i;
        for child in [2 * i + 1, 2 * i + 2] {
            if child < len {
                let (a, b) = (
                    item(order.name, items, child)?,
                    item(order.name, items, smallest)?,
                );
                if order.less(interp, &a, &b)? {
                    smallest = child;
                }
            }
        }
        if smallest == i {
            return Ok(());
        }
        swap(order.name, items, i, smallest)?;
        i = smallest;
    }
}

/// `cmp` の引数（`index` 番目。なくてもよい）
fn cmp_arg<'v>(name: &str, args: &'v [Value], index: usize) -> Result<Option<&'v Value>, String> {
    args.get(index)
        .map(|_| func_arg(name, args, index))
        .transpose()
}

fn heap_push(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "collections.heap_push";
    count_range(NAME, args, 2, 3)?;
    let heap = array_arg(NAME, args, 0)?;
    let order = Order {
        name: NAME,
        cmp: cmp_arg(NAME, args, 2)?,
    };
    let i = {
        let mut items = heap.borrow_mut();
        items.push(args[1].clone());
        items.len() - 1
    };
    sift_up(interp, &order, heap, i)?;
    Ok(Value::Nil)
}

fn heap_pop(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "collections.heap_pop";
    count_range(NAME, args, 1, 2)?;
    let heap = array_arg(NAME, args, 0)?;
    let order = Order {
        name: NAME,
        cmp: cmp_arg(NAME, args, 1)?,
    };
    let len = heap.borrow().len();
    if len == 0 {
        return Err("空のヒープからは取り出せません".into());
    }
    swap(NAME, heap, 0, len - 1)?;
    let smallest = heap.borrow_mut().pop().expect("長さを確かめた");
    sift_down(interp, &order, heap, 0)?;
    Ok(smallest)
}

fn heap_peek(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("collections.heap_peek", args, 1)?;
    array_arg("collections.heap_peek", args, 0)?
        .borrow()
        .first()
        .cloned()
        .ok_or_else(|| "空のヒープは見られません".to_string())
}

fn heapify(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "collections.heapify";
    count_range(NAME, args, 1, 2)?;
    let heap = array_arg(NAME, args, 0)?;
    let order = Order {
        name: NAME,
        cmp: cmp_arg(NAME, args, 1)?,
    };
    let len = heap.borrow().len();
    for i in (0..len / 2).rev() {
        sift_down(interp, &order, heap, i)?;
    }
    Ok(Value::Nil)
}

// ---- 数える・まとめる ----

/// `items` から重複を除いたものと、それぞれの個数（最初に出た順）。等しさは `==`
fn tally(items: &[Value]) -> Vec<(Value, usize)> {
    let mut seen: Vec<(Value, usize)> = Vec::new();
    for value in items {
        match seen.iter_mut().find(|(v, _)| v.equals(value)) {
            Some((_, count)) => *count += 1,
            None => seen.push((value.clone(), 1)),
        }
    }
    seen
}

fn counts(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("collections.counts", args, 1)?;
    let items = array_arg("collections.counts", args, 0)?.borrow().clone();
    let out = tally(&items)
        .into_iter()
        .map(|(value, count)| {
            let mut map = Map::default();
            map.insert("item".into(), value);
            map.insert("count".into(), Value::Num(count as f64));
            new_map(map)
        })
        .collect();
    Ok(new_array(out))
}

fn unique(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("collections.unique", args, 1)?;
    let items = array_arg("collections.unique", args, 0)?.borrow().clone();
    Ok(new_array(
        tally(&items).into_iter().map(|(value, _)| value).collect(),
    ))
}

fn group_by(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "collections.group_by";
    expect_args(NAME, args, 2)?;
    let items = array_arg(NAME, args, 0)?.borrow().clone();
    let f = func_arg(NAME, args, 1)?;
    let mut groups: Vec<(Rc<str>, Vec<Value>)> = Vec::new();
    for value in items {
        let key = match interp.call_value(f, vec![value.clone()])? {
            Value::Str(key) => key,
            other => {
                return Err(format!(
                    "{NAME} の関数は文字列を返す必要があります（{}でした）",
                    other.type_name()
                ));
            }
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(value),
            None => groups.push((key, vec![value])),
        }
    }
    let mut map = Map::default();
    for (key, members) in groups {
        map.insert(key, new_array(members));
    }
    Ok(new_map(map))
}

// ---- 配列の組み立て ----

fn chunk(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "collections.chunk";
    expect_args(NAME, args, 2)?;
    let items = array_arg(NAME, args, 0)?.borrow().clone();
    let n = int_arg(NAME, args, 1)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| format!("{NAME} の個数は 1 以上の整数です（{n}）"))?;
    Ok(new_array(
        items
            .chunks(n)
            .map(|part| new_array(part.to_vec()))
            .collect(),
    ))
}

fn zip(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("collections.zip", args, 2)?;
    let a = array_arg("collections.zip", args, 0)?.borrow().clone();
    let b = array_arg("collections.zip", args, 1)?.borrow().clone();
    Ok(new_array(
        a.into_iter()
            .zip(b)
            .map(|(x, y)| new_array(vec![x, y]))
            .collect(),
    ))
}

fn flatten(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("collections.flatten", args, 1)?;
    let items = array_arg("collections.flatten", args, 0)?.borrow().clone();
    let mut out = Vec::new();
    for value in items {
        match &value {
            Value::Array(inner) => out.extend(inner.borrow().iter().cloned()),
            _ => out.push(value),
        }
    }
    Ok(new_array(out))
}
