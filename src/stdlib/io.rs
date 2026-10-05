//! 入出力: log, input

use std::io::{self, BufRead};

use super::expect_args;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register("log", log);
    interp.register("input", input);
}

/// 引数を空白区切りで出力し、改行する
fn log(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let line: Vec<String> = args.iter().map(Value::to_string).collect();
    writeln!(interp.out, "{}", line.join(" ")).map_err(|e| e.to_string())?;
    Ok(Value::Nil)
}

/// 標準入力から 1 行読み、末尾の改行を除いて返す
fn input(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("input", args, 0)?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(Value::Str(line.trim_end_matches(['\r', '\n']).into()))
}
