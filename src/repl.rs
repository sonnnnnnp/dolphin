//! 対話モード（REPL）。`dolphin` を引数なしで起動すると使える
//!
//! - 式を入力すると値を表示する（文字列は `"` で囲む。値なしは表示しない）
//! - `(` や `"` が閉じていなければ、続きの行を `..` で読む
//! - Ctrl+C で入力中の内容を捨て、`exit` か Ctrl+D（Windows は Ctrl+Z）で終わる

use std::io;

use dolphin::Session;
use dolphin::runtime::value::{Repr, Value};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

/// 終了コードを返す
pub fn run() -> i32 {
    let mut editor = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("端末を初期化できませんでした: {e}");
            return 1;
        }
    };
    let mut stdout = io::stdout();
    let mut session = Session::new(&mut stdout);
    println!("Dolphin {} — exit で終了", env!("CARGO_PKG_VERSION"));

    // 複数行にまたがる入力をためる
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() { ">> " } else { ".. " };
        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => return 0,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        };
        if buffer.is_empty() && line.trim() == "exit" {
            return 0;
        }
        buffer.push_str(&line);
        buffer.push('\n');

        match session.eval(&buffer) {
            Ok(Some(value)) if !matches!(value, Value::Nil) => println!("{}", Repr(&value)),
            Ok(_) => {}
            // 入力の途中なので、続きを読む
            Err(diag) if diag.unexpected_eof => continue,
            Err(diag) => eprint!("{}", diag.render("<repl>", &buffer)),
        }
        // 履歴への追加に失敗しても REPL は続けられる
        let _ = editor.add_history_entry(buffer.trim_end());
        buffer.clear();
    }
}
