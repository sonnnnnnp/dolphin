//! `std:ui` の部品を作る関数（design.md 5.24）。部品は、`type` キーを持つマップ

use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;
use crate::stdlib::{expect_args, func_arg, new_map, num_arg, str_arg, type_error};

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:ui",
        &[
            ("ui.run", super::run),
            ("ui.refresh", super::refresh),
            ("ui.quit", super::quit),
            ("ui.text", text),
            ("ui.button", button),
            ("ui.text_input", text_input),
            ("ui.checkbox", checkbox),
            ("ui.slider", slider),
            ("ui.select", select),
            ("ui.progress", progress),
            ("ui.column", |_, a| layout("ui.column", "column", a)),
            ("ui.row", |_, a| layout("ui.row", "row", a)),
            ("ui.group", group),
            ("ui.scroll", scroll),
            ("ui.separator", separator),
            ("ui.space", space),
        ],
    );
}

/// オプションのキーの型
#[derive(Clone, Copy)]
enum Ty {
    Num,
    Str,
    Bool,
    /// `#rrggbb` の文字列
    Color,
}

fn describe(ty: Ty) -> &'static str {
    match ty {
        Ty::Num => "数値",
        Ty::Str => "文字列",
        Ty::Bool => "真偽値",
        Ty::Color => "`#rrggbb` の文字列",
    }
}

/// `args[i]`（省略できる）の `options` を検査して、(キー, 値) の並びにする
fn options(
    name: &str,
    args: &[Value],
    i: usize,
    known: &[(&str, Ty)],
) -> Result<Vec<(String, Value)>, String> {
    let Some(value) = args.get(i) else {
        return Ok(Vec::new());
    };
    let Value::Map(map) = value else {
        return Err(type_error(name, i, "マップ", value));
    };
    let mut out = Vec::new();
    for (key, value) in map.borrow().iter() {
        let Some((_, ty)) = known.iter().find(|(k, _)| *k == &**key) else {
            let names: Vec<&str> = known.iter().map(|(k, _)| *k).collect();
            return Err(format!(
                "{name} に知らないオプション `{key}` があります（使えるのは {}）",
                names.join("、")
            ));
        };
        let ok = match (ty, value) {
            (Ty::Num, Value::Num(n)) => n.is_finite(),
            (Ty::Str, Value::Str(_)) | (Ty::Bool, Value::Bool(_)) => true,
            (Ty::Color, Value::Str(s)) => super::parse_color(s).is_some(),
            _ => false,
        };
        if !ok {
            return Err(format!(
                "{name} のオプション `{key}` は{}です（{}が渡されました）",
                describe(*ty),
                value.type_name()
            ));
        }
        out.push((key.to_string(), value.clone()));
    }
    Ok(out)
}

/// `type` と、そのほかのキーを持つ部品のマップ
fn widget(kind: &str, fields: Vec<(&str, Value)>, extra: Vec<(String, Value)>) -> Value {
    let mut map = Map::default();
    map.insert("type".into(), Value::Str(kind.into()));
    for (key, value) in fields {
        map.insert(key.into(), value);
    }
    for (key, value) in extra {
        map.insert(key.as_str().into(), value);
    }
    new_map(map)
}

fn args_between(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else if min == max {
        expect_args(name, args, min)
    } else {
        Err(format!(
            "{name} の引数は {min} 個から {max} 個です（{} 個渡されました）",
            args.len()
        ))
    }
}

fn text(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("ui.text", args, 1, 2)?;
    // 文字列以外も、表示用の文字列にする（`ui.text[@count]`）
    let text = args[0].to_string();
    let extra = options(
        "ui.text",
        args,
        1,
        &[("size", Ty::Num), ("color", Ty::Color), ("bold", Ty::Bool)],
    )?;
    Ok(widget(
        "text",
        vec![("text", Value::Str(text.into()))],
        extra,
    ))
}

fn button(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("ui.button", args, 2, 3)?;
    let label = args[0].to_string();
    let on_click = func_arg("ui.button", args, 1)?.clone();
    let extra = options("ui.button", args, 2, &[("enabled", Ty::Bool)])?;
    Ok(widget(
        "button",
        vec![("label", Value::Str(label.into())), ("on_click", on_click)],
        extra,
    ))
}

fn text_input(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("ui.text_input", args, 2, 3)?;
    let value = str_arg("ui.text_input", args, 0)?;
    let on_change = func_arg("ui.text_input", args, 1)?.clone();
    let extra = options(
        "ui.text_input",
        args,
        2,
        &[
            ("hint", Ty::Str),
            ("multiline", Ty::Bool),
            ("password", Ty::Bool),
        ],
    )?;
    Ok(widget(
        "text_input",
        vec![
            ("value", Value::Str(value.into())),
            ("on_change", on_change),
        ],
        extra,
    ))
}

fn checkbox(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.checkbox", args, 3)?;
    let label = args[0].to_string();
    let Value::Bool(checked) = args[1] else {
        return Err(type_error("ui.checkbox", 1, "真偽値", &args[1]));
    };
    let on_change = func_arg("ui.checkbox", args, 2)?.clone();
    Ok(widget(
        "checkbox",
        vec![
            ("label", Value::Str(label.into())),
            ("checked", Value::Bool(checked)),
            ("on_change", on_change),
        ],
        Vec::new(),
    ))
}

fn slider(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.slider", args, 4)?;
    let (value, min, max) = (
        num_arg("ui.slider", args, 0)?,
        num_arg("ui.slider", args, 1)?,
        num_arg("ui.slider", args, 2)?,
    );
    if !(value.is_finite() && min.is_finite() && max.is_finite() && min < max) {
        return Err(format!(
            "ui.slider の範囲は min < max の有限の数です（min {min}、max {max}）"
        ));
    }
    let on_change = func_arg("ui.slider", args, 3)?.clone();
    Ok(widget(
        "slider",
        vec![
            ("value", Value::Num(value)),
            ("min", Value::Num(min)),
            ("max", Value::Num(max)),
            ("on_change", on_change),
        ],
        Vec::new(),
    ))
}

fn select(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.select", args, 3)?;
    let Value::Array(labels) = &args[0] else {
        return Err(type_error("ui.select", 0, "配列", &args[0]));
    };
    let count = labels.borrow().len();
    let index = num_arg("ui.select", args, 1)?;
    if index.fract() != 0.0 || index < 0.0 || index >= count as f64 {
        return Err(format!(
            "ui.select の番号は 0〜{} の整数です（{index}）",
            count.saturating_sub(1)
        ));
    }
    let on_change = func_arg("ui.select", args, 2)?.clone();
    Ok(widget(
        "select",
        vec![
            ("labels", args[0].clone()),
            ("index", Value::Num(index)),
            ("on_change", on_change),
        ],
        Vec::new(),
    ))
}

fn progress(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.progress", args, 1)?;
    let fraction = num_arg("ui.progress", args, 0)?;
    Ok(widget(
        "progress",
        vec![("fraction", Value::Num(fraction.clamp(0.0, 1.0)))],
        Vec::new(),
    ))
}

fn children_arg(name: &str, args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        children @ Value::Array(_) => Ok(children.clone()),
        other => Err(type_error(name, 0, "配列", other)),
    }
}

fn layout(name: &str, kind: &str, args: &[Value]) -> Result<Value, String> {
    args_between(name, args, 1, 2)?;
    let children = children_arg(name, args)?;
    let extra = options(name, args, 1, &[("spacing", Ty::Num)])?;
    Ok(widget(kind, vec![("children", children)], extra))
}

fn group(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.group", args, 1)?;
    let children = children_arg("ui.group", args)?;
    Ok(widget("group", vec![("children", children)], Vec::new()))
}

fn scroll(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.scroll", args, 1)?;
    Ok(widget(
        "scroll",
        vec![("child", args[0].clone())],
        Vec::new(),
    ))
}

fn separator(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.separator", args, 0)?;
    Ok(widget("separator", Vec::new(), Vec::new()))
}

fn space(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.space", args, 1)?;
    let size = num_arg("ui.space", args, 0)?;
    if !(size.is_finite() && size >= 0.0) {
        return Err(format!("ui.space の大きさは 0 以上の数です（{size}）"));
    }
    Ok(widget(
        "space",
        vec![("size", Value::Num(size))],
        Vec::new(),
    ))
}
