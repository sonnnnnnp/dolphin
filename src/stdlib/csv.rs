//! `std:csv`: parse, stringify（design.md 5.29）。標準ライブラリだけで書く

use std::rc::Rc;

use super::{array_arg, error_value, map_arg, new_array, new_map, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:csv",
        &[("csv.parse", parse), ("csv.stringify", stringify)],
    );
}

// ---- オプション ----

struct Options {
    delimiter: char,
    header: Option<bool>,
    columns: Option<Vec<Rc<str>>>,
    newline: &'static str,
}

/// `allowed` に挙げたキーだけを受け付ける。間違いはプログラムの間違い（実行時エラー）
fn options_arg(name: &str, args: &[Value], allowed: &[&str]) -> Result<Options, String> {
    let mut options = Options {
        delimiter: ',',
        header: None,
        columns: None,
        newline: "\n",
    };
    if args.len() < 2 {
        return Ok(options);
    }
    let map = map_arg(name, args, 1)?;
    for (key, value) in map.borrow().iter() {
        if !allowed.contains(&&**key) {
            return Err(format!("{name} のオプション `{key}` は知りません"));
        }
        let bad_type = |expected: &str| {
            format!(
                "{name} のオプション `{key}` は{expected}です（{}が渡されました）",
                value.type_name()
            )
        };
        match (&**key, value) {
            ("delimiter", Value::Str(s)) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if !matches!(c, '"' | '\n' | '\r') => options.delimiter = c,
                    _ => {
                        return Err(format!(
                            "{name} の `delimiter` は、`\"` と改行以外の 1 文字です（{s:?}）"
                        ));
                    }
                }
            }
            ("header", Value::Bool(b)) => options.header = Some(*b),
            ("columns", Value::Array(items)) => {
                let mut columns = Vec::new();
                for item in items.borrow().iter() {
                    let Value::Str(s) = item else {
                        return Err(bad_type("文字列の配列"));
                    };
                    columns.push(Rc::clone(s));
                }
                options.columns = Some(columns);
            }
            ("newline", Value::Str(s)) => {
                options.newline = match &**s {
                    "\n" => "\n",
                    "\r\n" => "\r\n",
                    _ => {
                        return Err(format!(
                            "{name} の `newline` は \"\\n\" か \"\\r\\n\" です（{s:?}）"
                        ));
                    }
                };
            }
            ("delimiter" | "newline", _) => return Err(bad_type("文字列")),
            ("header", _) => return Err(bad_type("真偽値")),
            _ => return Err(bad_type("配列")),
        }
    }
    Ok(options)
}

// ---- 読む ----

/// 1 行ぶん。`line` は、その行が始まった行番号（1 始まり）
struct Row {
    line: usize,
    fields: Vec<String>,
}

fn parse_rows(text: &str, delimiter: char) -> Result<Vec<Row>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut chars = text.chars().peekable();
    let mut rows = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut at_start = true;
    let mut line = 1;
    let mut row_line = 1;

    // 空の行（空のフィールドが 1 つだけで、`"` で囲まれていない）は飛ばす
    let finish_row = |rows: &mut Vec<Row>,
                      fields: &mut Vec<String>,
                      field: &mut String,
                      quoted: &mut bool,
                      row_line: usize| {
        if !fields.is_empty() || !field.is_empty() || *quoted {
            fields.push(std::mem::take(field));
            rows.push(Row {
                line: row_line,
                fields: std::mem::take(fields),
            });
        }
        *quoted = false;
    };

    while let Some(c) = chars.next() {
        match c {
            '"' if at_start => {
                let start = line;
                quoted = true;
                loop {
                    match chars.next() {
                        None => {
                            return Err(format!("{start} 行目: 閉じていない `\"` があります"));
                        }
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            field.push('"');
                        }
                        Some('"') => break,
                        Some(ch) => {
                            if ch == '\n' {
                                line += 1;
                            }
                            field.push(ch);
                        }
                    }
                }
                at_start = false;
                match chars.peek() {
                    None | Some('\n' | '\r') => {}
                    Some(&next) if next == delimiter => {}
                    Some(_) => {
                        return Err(format!(
                            "{line} 行目: 閉じた `\"` の後に余計な文字があります"
                        ));
                    }
                }
            }
            c if c == delimiter => {
                fields.push(std::mem::take(&mut field));
                at_start = true;
                quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                finish_row(&mut rows, &mut fields, &mut field, &mut quoted, row_line);
                at_start = true;
                line += 1;
                row_line = line;
            }
            other => {
                field.push(other);
                at_start = false;
            }
        }
    }
    finish_row(&mut rows, &mut fields, &mut field, &mut quoted, row_line);
    Ok(rows)
}

fn parse(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "csv.parse の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let text = str_arg("csv.parse", args, 0)?;
    let options = options_arg("csv.parse", args, &["delimiter", "header"])?;
    let rows = match parse_rows(text, options.delimiter) {
        Ok(rows) => rows,
        Err(message) => return Ok(error_value("parse", message)),
    };
    let strings = |fields: Vec<String>| -> Vec<Value> {
        fields.into_iter().map(|s| Value::Str(s.into())).collect()
    };
    if options.header != Some(true) {
        let rows = rows
            .into_iter()
            .map(|row| new_array(strings(row.fields)))
            .collect();
        return Ok(new_array(rows));
    }

    let mut rows = rows.into_iter();
    let Some(head) = rows.next() else {
        return Ok(new_array(Vec::new()));
    };
    let headers: Vec<Rc<str>> = head.fields.into_iter().map(Rc::from).collect();
    let mut out = Vec::new();
    for row in rows {
        if row.fields.len() > headers.len() {
            return Ok(error_value(
                "parse",
                format!(
                    "{} 行目: 列の数（{}）が見出し（{}）より多いです",
                    row.line,
                    row.fields.len(),
                    headers.len()
                ),
            ));
        }
        let mut fields = row.fields.into_iter();
        let mut map = Map::default();
        for header in &headers {
            let value = fields.next().map_or(Value::Nil, |s| Value::Str(s.into()));
            map.insert(Rc::clone(header), value);
        }
        out.push(new_map(map));
    }
    Ok(new_array(out))
}

// ---- 書く ----

fn field_text(value: &Value) -> Result<String, String> {
    match value {
        Value::Nil => Ok(String::new()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Num(n) => Ok(n.to_string()),
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!(
            "csv.stringify で書けるのは nil・真偽値・数値・文字列です（{}が渡されました）",
            other.type_name()
        )),
    }
}

fn push_field(out: &mut String, text: &str, delimiter: char) {
    if text.contains([delimiter, '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&text.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(text);
    }
}

fn push_row(out: &mut String, fields: &[String], options: &Options) {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push(options.delimiter);
        }
        push_field(out, field, options.delimiter);
    }
    out.push_str(options.newline);
}

fn stringify(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "csv.stringify の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let rows = array_arg("csv.stringify", args, 0)?.borrow();
    let options = options_arg(
        "csv.stringify",
        args,
        &["delimiter", "header", "columns", "newline"],
    )?;
    let mut out = String::new();
    let Some(first) = rows.first() else {
        return Ok(Value::Str(out.into()));
    };

    match first {
        Value::Array(_) => {
            for (i, row) in rows.iter().enumerate() {
                let Value::Array(items) = row else {
                    return Err(mixed_rows(i, row));
                };
                let fields = items
                    .borrow()
                    .iter()
                    .map(field_text)
                    .collect::<Result<Vec<_>, _>>()?;
                push_row(&mut out, &fields, &options);
            }
        }
        Value::Map(first) => {
            let columns: Vec<Rc<str>> = match &options.columns {
                Some(columns) => columns.clone(),
                None => first.borrow().iter().map(|(k, _)| Rc::clone(k)).collect(),
            };
            if options.header != Some(false) {
                let names: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
                push_row(&mut out, &names, &options);
            }
            for (i, row) in rows.iter().enumerate() {
                let Value::Map(map) = row else {
                    return Err(mixed_rows(i, row));
                };
                let map = map.borrow();
                let fields = columns
                    .iter()
                    .map(|c| map.get(c).map_or(Ok(String::new()), field_text))
                    .collect::<Result<Vec<_>, _>>()?;
                push_row(&mut out, &fields, &options);
            }
        }
        other => return Err(type_error("csv.stringify", 0, "配列かマップの配列", other)),
    }
    Ok(Value::Str(out.into()))
}

fn mixed_rows(i: usize, row: &Value) -> String {
    format!(
        "csv.stringify の行は、すべて配列か、すべてマップです（{} 行目が{}）",
        i + 1,
        row.type_name()
    )
}
