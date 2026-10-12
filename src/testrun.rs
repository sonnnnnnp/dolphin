//! `dolphin test`（design.md 5.17）

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Session;

/// `paths`（ファイルかディレクトリ。空なら `.`）のテストを実行して、結果を `report` に書く。
/// 終了コード（すべて成功なら 0）を返す
pub fn run(paths: &[PathBuf], report: &mut dyn Write) -> i32 {
    let default = [PathBuf::from(".")];
    let paths = if paths.is_empty() { &default } else { paths };
    let mut files = Vec::new();
    for path in paths {
        if let Err(e) = collect(path, &mut files) {
            let _ = writeln!(report, "{}: {e}", path.display());
            return 1;
        }
    }

    let mut total = Tally::default();
    for file in &files {
        run_file(file, report, &mut total);
    }
    if total.passed + total.failed == 0 && total.broken == 0 {
        let _ = writeln!(report, "テストが見つかりません");
        return 1;
    }
    let mut summary = format!("{} 件成功、{} 件失敗", total.passed, total.failed);
    if total.broken > 0 {
        summary += &format!("、{} ファイルが実行できませんでした", total.broken);
    }
    let _ = writeln!(report, "\n{summary}");
    i32::from(total.failed > 0 || total.broken > 0)
}

#[derive(Default)]
struct Tally {
    passed: usize,
    failed: usize,
    /// 本体のエラーで、テストまで進めなかったファイル
    broken: usize,
}

/// ファイルはそのまま、ディレクトリは中の `*_test.dol` を集める（名前順）
fn collect(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !fs::metadata(path)?.is_dir() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for entry in entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if entry.is_dir() {
            // 隠しディレクトリとビルド結果は見ない
            if !name.starts_with('.') && name != "target" {
                collect(&entry, files)?;
            }
        } else if name.ends_with("_test.dol") {
            files.push(entry);
        }
    }
    Ok(())
}

fn run_file(path: &Path, report: &mut dyn Write, total: &mut Tally) {
    let name = path.display().to_string();
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            let _ = writeln!(report, "{name}\n  {e}");
            total.broken += 1;
            return;
        }
    };
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();

    let mut output = Vec::new();
    let mut lines = String::new();
    let mut failed_here = false;
    {
        let mut session = Session::with_base_dir(&mut output, dir);
        match session.run(&src) {
            Err(diag) => {
                lines += &diag.render(&name, &src);
                total.broken += 1;
                failed_here = true;
            }
            Ok(()) => {
                for (func, arity) in session.user_functions() {
                    if arity != 0 || !func.starts_with("test_") {
                        continue;
                    }
                    if session.exit_code().is_some() {
                        break;
                    }
                    match session.run(&format!("{func}[]")) {
                        Ok(()) => {
                            total.passed += 1;
                            lines += &format!("  ok   {func}\n");
                        }
                        Err(diag) => {
                            total.failed += 1;
                            failed_here = true;
                            lines += &format!("  FAIL {func}\n");
                            for line in diag.render(&name, &src).lines() {
                                lines += &format!("       {line}\n");
                            }
                        }
                    }
                }
                if session.exit_code().is_some() {
                    total.failed += 1;
                    failed_here = true;
                    lines += "  FAIL os.exit が呼ばれました（残りのテストは実行しません）\n";
                }
            }
        }
    }
    let _ = writeln!(report, "{name}");
    let _ = write!(report, "{lines}");
    if failed_here && !output.is_empty() {
        let _ = writeln!(report, "  --- 出力 ---");
        for line in String::from_utf8_lossy(&output).lines() {
            let _ = writeln!(report, "  {line}");
        }
    }
}
