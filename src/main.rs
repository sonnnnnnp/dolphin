use std::{env, fs, io, process};

use dolphin::syntax::{lexer, parser};

enum Mode {
    Run,
    /// トークン列を表示する
    Tokens,
    /// AST を S 式で表示する
    Ast,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (mode, path) = match args.as_slice() {
        [flag, path] if flag == "--tokens" => (Mode::Tokens, path),
        [flag, path] if flag == "--ast" => (Mode::Ast, path),
        [path] => (Mode::Run, path),
        _ => {
            eprintln!("使い方: dolphin [--tokens | --ast] <file.dol>");
            process::exit(2);
        }
    };
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("{path}: {e}");
            process::exit(1);
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
    if let Err(diag) = result {
        eprint!("{}", diag.render(path, &src));
        process::exit(1);
    }
}
