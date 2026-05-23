use macroquad::prelude::*;
mod interpreter;
use interpreter::Interpreter;

fn window_conf() -> Conf {
    Conf {
        window_title: "dolphin".to_owned(),
        window_width: 800,
        window_height: 600,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut interp = Interpreter::new();

    if args.len() > 1 {
        let code = match std::fs::read_to_string(&args[1]) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("ファイルを開けませんでした: {}", args[1]);
                return;
            }
        };
        interp.run(&code).await;
    } else {
        println!("Dolphin REPL モード。終了するには 'exit' を入力してください。");
        use std::io::{self, BufRead, Write};
        let stdin = io::stdin();
        loop {
            print!("> ");
            io::stdout().flush().ok();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).is_err() {
                break;
            }
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            if line == "exit" {
                break;
            }
            interp.run(line).await;
        }
    }
}
