use std::path::{Path, PathBuf};
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

/// tree-walking 評価は Dolphin の関数呼び出し 1 段ごとに Rust の再帰が何段も深くなる。
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
    match args[0].as_str() {
        "check" => return check(&args[1..]),
        "fmt" => return fmt(&args[1..]),
        "test" => {
            let paths: Vec<_> = args[1..].iter().map(PathBuf::from).collect();
            return dolphin::testrun::run(&paths, &mut io::stdout());
        }
        _ => {}
    }
    let (mode, path, script_args) = match args.as_slice() {
        [flag, path] if flag == "--tokens" => (Mode::Tokens, path, &[][..]),
        [flag, path] if flag == "--ast" => (Mode::Ast, path, &[][..]),
        [flag, ..] if flag.starts_with("--") => {
            eprintln!("使い方: dolphin [--tokens | --ast] <file.dol>");
            eprintln!("        dolphin <file.dol> [引数...]");
            eprintln!("        dolphin check <file.dol>...  （実行せずに検査）");
            eprintln!("        dolphin test [パス...]       （*_test.dol の test_ 関数を実行）");
            eprintln!("        dolphin fmt [--check | --stdout] <file.dol>...  （整形）");
            eprintln!("        dolphin              （引数なしで対話モード）");
            return 2;
        }
        [path, rest @ ..] => (Mode::Run, path, rest),
        [] => unreachable!("引数なしは対話モードで処理済み"),
    };
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 1;
        }
    };

    let result = match mode {
        Mode::Run => {
            let dir = Path::new(path).parent().unwrap_or(Path::new(""));
            let args = script_args.to_vec();
            match dolphin::run_script(&src, dir.to_path_buf(), args, &mut io::stdout()) {
                Ok(0) => Ok(()),
                // `os.exit` で終わった
                Ok(code) => return code,
                Err(diag) => Err(diag),
            }
        }
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

/// `dolphin fmt`: 整形して書き戻す。`--check` は書き換えず、整形が必要なら 1。
/// `--stdout` は結果を標準出力に出す（1 ファイル）
fn fmt(args: &[String]) -> i32 {
    let (check_only, to_stdout) = (
        args.iter().any(|a| a == "--check"),
        args.iter().any(|a| a == "--stdout"),
    );
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if paths.is_empty()
        || (check_only && to_stdout)
        || (to_stdout && paths.len() != 1)
        || args
            .iter()
            .any(|a| a.starts_with("--") && a != "--check" && a != "--stdout")
    {
        eprintln!("使い方: dolphin fmt [--check | --stdout] <file.dol>...");
        return 2;
    }
    let mut code = 0;
    for path in paths {
        let src = match fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("{path}: {e}");
                code = 1;
                continue;
            }
        };
        let formatted = match dolphin::fmt::format_source(&src) {
            Ok(formatted) => formatted,
            Err(dolphin::fmt::FmtError::Syntax(diag)) => {
                eprint!("{}", diag.render(path, &src));
                code = 1;
                continue;
            }
            Err(dolphin::fmt::FmtError::Internal(message)) => {
                eprintln!("{path}: 内部エラー: {message}");
                code = 1;
                continue;
            }
        };
        if to_stdout {
            print!("{formatted}");
        } else if formatted != src {
            if check_only {
                println!("{path}");
                code = 1;
            } else if let Err(e) = fs::write(path, &formatted) {
                eprintln!("{path}: {e}");
                code = 1;
            } else {
                println!("{path}");
            }
        }
    }
    code
}

/// `dolphin check`: 実行せずに構文と Resolver の検査をする。問題があれば 1
fn check(paths: &[String]) -> i32 {
    if paths.is_empty() {
        eprintln!("使い方: dolphin check <file.dol>...");
        return 2;
    }
    let mut code = 0;
    for path in paths {
        let src = match fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("{path}: {e}");
                code = 1;
                continue;
            }
        };
        let dir = Path::new(path).parent().unwrap_or(Path::new(""));
        let mut out = io::sink();
        if let Err(diag) = dolphin::Session::with_base_dir(&mut out, dir.to_path_buf()).check(&src)
        {
            eprint!("{}", diag.render(path, &src));
            code = 1;
        }
    }
    code
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
