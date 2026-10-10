//! tests/cases/*.dol を実行し、出力を同名の *.out と比較する
//! エラーになるケースは、エラー表示も含めて *.out に書く

use std::fs;
use std::path::Path;

#[test]
fn golden() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut failures = Vec::new();

    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "dol") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let expected = fs::read_to_string(path.with_extension("out")).unwrap_or_default();

        let mut out = Vec::new();
        let result = dolphin::run_source_in(&src, dir.clone(), &mut out);
        let mut actual = String::from_utf8(out).unwrap();
        if let Err(diag) = result {
            actual += &diag.render(&name, &src);
        }

        if actual.replace("\r\n", "\n") != expected.replace("\r\n", "\n") {
            failures.push(format!("--- {name}\n期待:\n{expected}\n実際:\n{actual}"));
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
