//! リポジトリの `.dol` ファイルを整形して、字句が変わらず、冪等であることを確かめる

use std::fs;
use std::path::Path;

use dolphin::fmt::{FmtError, format_source};

#[test]
fn formatting_repo_sources_is_safe_and_idempotent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for dir in ["examples", "tests/cases"] {
        for entry in fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "dol") {
                continue;
            }
            let src = fs::read_to_string(&path).unwrap();
            match format_source(&src) {
                Ok(once) => {
                    let twice = format_source(&once).unwrap_or_else(|e| {
                        panic!(
                            "{}: 整形後のファイルを整形できません: {e:?}",
                            path.display()
                        )
                    });
                    assert_eq!(once, twice, "{}: 冪等ではありません", path.display());
                    checked += 1;
                }
                // 構文エラーを確かめるケースは整形しない
                Err(FmtError::Syntax(_)) => {}
                Err(e) => panic!("{}: {e:?}", path.display()),
            }
        }
    }
    assert!(checked > 100, "整形できたファイルが少なすぎます: {checked}");
}
