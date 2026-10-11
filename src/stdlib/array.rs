//! 配列: arr_len, arr_push, arr_pop, arr_map, arr_filter, arr_each, range

use super::{array_arg, expect_args, func_arg, int_arg, new_array};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("arr_len", arr_len);
    interp.register("arr_push", arr_push);
    interp.register("arr_pop", arr_pop);
    interp.register("arr_map", arr_map);
    interp.register("arr_filter", arr_filter);
    interp.register("arr_each", arr_each);
    interp.register("range", range);
}

/// `range` が作る配列の要素数の上限
const RANGE_MAX: i64 = 10_000_000;

/// `a` 以上 `b` 未満の整数の配列
fn range(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("range", args, 2)?;
    let (a, b) = (int_arg("range", args, 0)?, int_arg("range", args, 1)?);
    if b.saturating_sub(a) > RANGE_MAX {
        return Err(format!("range の要素数が多すぎます（{RANGE_MAX} 個まで）"));
    }
    Ok(new_array((a..b).map(|n| Value::Num(n as f64)).collect()))
}

fn arr_len(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_len", args, 1)?;
    let items = array_arg("arr_len", args, 0)?;
    Ok(Value::Num(items.borrow().len() as f64))
}

/// 末尾に追加する。配列は参照渡しなので、呼び出し元の配列が変わる（design.md 7.1）
fn arr_push(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_push", args, 2)?;
    let items = array_arg("arr_push", args, 0)?;
    items.borrow_mut().push(args[1].clone());
    Ok(Value::Nil)
}

/// 末尾を取り除いて返す。空ならエラー
fn arr_pop(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_pop", args, 1)?;
    let items = array_arg("arr_pop", args, 0)?;
    items
        .borrow_mut()
        .pop()
        .ok_or_else(|| "空の配列からは取り出せません".to_string())
}

/// 各要素に `f` を適用した新しい配列。呼んでいる間に元の配列が書き換えられても、
/// 呼び始めたときの要素を使う
fn arr_map(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_map", args, 2)?;
    let items = array_arg("arr_map", args, 0)?.borrow().clone();
    let f = func_arg("arr_map", args, 1)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(interp.call_value(f, vec![item])?);
    }
    Ok(new_array(out))
}

/// `f` が `true` を返した要素だけの新しい配列
fn arr_filter(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_filter", args, 2)?;
    let items = array_arg("arr_filter", args, 0)?.borrow().clone();
    let f = func_arg("arr_filter", args, 1)?;
    let mut out = Vec::new();
    for item in items {
        match interp.call_value(f, vec![item.clone()])? {
            Value::Bool(true) => out.push(item),
            Value::Bool(false) => {}
            other => {
                return Err(format!(
                    "arr_filter の関数は真偽値を返す必要があります（{}でした）",
                    other.type_name()
                ));
            }
        }
    }
    Ok(new_array(out))
}

/// 各要素に `f` を適用する。`f` の戻り値は捨てる
fn arr_each(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("arr_each", args, 2)?;
    let items = array_arg("arr_each", args, 0)?.borrow().clone();
    let f = func_arg("arr_each", args, 1)?;
    for item in items {
        interp.call_value(f, vec![item])?;
    }
    Ok(Value::Nil)
}
