//! 標準ライブラリ（ネイティブ関数）。モジュールごとに register で登録する

pub mod array;
pub mod io;
pub mod string;

use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    io::register(interp);
    string::register(interp);
    array::register(interp);
}

fn expect_args(name: &str, args: &[Value], n: usize) -> Result<(), String> {
    if args.len() == n {
        Ok(())
    } else {
        Err(format!(
            "{name} の引数は {n} 個です（{} 個渡されました）",
            args.len()
        ))
    }
}

fn type_error(name: &str, expected: &str, got: &Value) -> String {
    format!(
        "{name} の引数は{expected}です（{}が渡されました）",
        got.type_name()
    )
}
