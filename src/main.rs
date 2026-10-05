use std::{env, fs, io, process};

use dolphin::syntax::lexer;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (dump_tokens, path) = match args.as_slice() {
        [flag, path] if flag == "--tokens" => (true, path),
        [path] => (false, path),
        _ => {
            eprintln!("使い方: dolphin [--tokens] <file.dol>");
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

    let result = if dump_tokens {
        lexer::tokenize(&src).map(|tokens| {
            for token in tokens {
                println!("{}:{}\t{:?}", token.span.line, token.span.col, token.kind);
            }
        })
    } else {
        dolphin::run_source(&src, &mut io::stdout())
    };
    if let Err(diag) = result {
        eprint!("{}", diag.render(path, &src));
        process::exit(1);
    }
}
