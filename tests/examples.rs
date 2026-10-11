//! `examples/` のスクリプトが動くことを確かめる。ネットワークを使うものは、構文だけを調べる

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// リポジトリのルートで `dolphin` を動かす
fn dolphin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dolphin"))
        .args(args)
        .current_dir(root())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn every_example_parses() {
    let mut count = 0;
    for entry in fs::read_dir(root().join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "dol") {
            continue;
        }
        let name = format!("examples/{}", path.file_name().unwrap().to_string_lossy());
        let output = dolphin(&["--ast", &name]);
        assert!(output.status.success(), "{name}: {}", stderr(&output));
        count += 1;
    }
    assert!(count >= 6, "examples が少なすぎる: {count}");
}

#[test]
fn mandelbrot_draws_the_set() {
    let output = dolphin(&["examples/mandelbrot.dol"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert_eq!(text.lines().count(), 30);
    assert!(text.lines().all(|l| l.chars().count() == 78));
    // 実軸（16 行目）が対称軸で、上下が同じ絵になる。実軸の上には長い塊がある
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows[14], rows[16]);
    assert_eq!(rows[5], rows[25]);
    assert!(rows[15].contains("@@@@@@@@@@@@@@@@@@@@@@@"));
    assert!(rows[0].trim().is_empty());
}

#[test]
fn life_runs_a_few_generations() {
    let output = dolphin(&["examples/life.dol", "3"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("世代 0   生きているセル 36"));
    assert!(text.contains("世代 2"));
    assert!(!text.contains("世代 3"));

    let output = dolphin(&["examples/life.dol", "many"]);
    assert_eq!(output.status.code(), Some(2));
}

// dolvc は std:crypto（`crypto` feature）を使う
#[cfg(feature = "crypto")]
#[test]
fn dolvc_demo_commits_and_restores() {
    let output = dolphin(&["examples/dolvc_demo.dol"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for expected in [
        "空のリポジトリを作りました（.dolvc）",
        "（3 ファイル）",
        "  変更  hello.txt",
        "  追加  src/lib.dol",
        "  削除  logo.bin",
        "+ よろしくね",
        "（バイナリファイル）",
        "最初のコミット",
        "変更はありません",
        "logo.bin の中身: <bytes 89504e4700ff>",
    ] {
        assert!(text.contains(expected), "{expected:?} がない:\n{text}");
    }
}

#[cfg(feature = "crypto")]
#[test]
fn dolvc_usage() {
    let output = dolphin(&["examples/dolvc.dol"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).contains("使い方"));
}
