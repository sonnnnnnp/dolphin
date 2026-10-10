use std::{env, fs, io, process, thread};

use dolphin::syntax::{lexer, parser};

#[cfg(feature = "repl")]
mod repl;

enum Mode {
    Run,
    /// トークン列を表示する
    Tokens,
    /// AST を S 式で表示する
    Ast,
}

/// 木たどり評価は Dolphin の関数呼び出し 1 段ごとに Rust の再帰が何段も深くなる。
/// MAX_DEPTH 段まで呼んでもあふれないよう、大きめのスタックのスレッドで動かす
const STACK_SIZE: usize = 256 * 1024 * 1024;

fn main() {
    let cli = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(run_cli)
        .expect("スレッドを作れませんでした");
    // パニックした場合、メッセージはすでに表示されている
    process::exit(cli.join().unwrap_or(101));
}

/// 終了コードを返す
fn run_cli() -> i32 {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return start_repl();
    }
    let (mode, path) = match args.as_slice() {
        [flag, path] if flag == "--tokens" => (Mode::Tokens, path),
        [flag, path] if flag == "--ast" => (Mode::Ast, path),
        [path] => (Mode::Run, path),
        _ => {
            eprintln!("使い方: dolphin [--tokens | --ast] <file.dol>");
            eprintln!("        dolphin              （引数なしで対話モード）");
            return 2;
        }
    };
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 1;
        }
    };

    let result = match mode {
        Mode::Run => dolphin::run_source(&src, &mut io::stdout()),
        Mode::Tokens => lexer::tokenize(&src).map(|tokens| {
            for token in tokens {
                println!("{}:{}\t{:?}", token.span.line, token.span.col, token.kind);
            }
        }),
        Mode::Ast => lexer::tokenize(&src)
            .and_then(|tokens| parser::parse(&tokens))
            .map(|program| print!("{program}")),
    };
    match result {
        Ok(()) => 0,
        Err(diag) => {
            eprint!("{}", diag.render(path, &src));
            1
        }
    }
}

#[cfg(feature = "repl")]
fn start_repl() -> i32 {
    repl::run()
}

#[cfg(not(feature = "repl"))]
fn start_repl() -> i32 {
    eprintln!("対話モードを使うには repl 機能を有効にしてビルドしてください");
    2
}
