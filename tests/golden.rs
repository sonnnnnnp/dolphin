//! tests/cases/*.dol を実行し、出力を同名の *.out と比較する
//! エラーになるケースは、エラー表示も含めて *.out に書く。
//! `http` feature を外したときだけ出力が変わるケースは、`名前.nohttp.out` に書く。
//! 名前が `crypto` で始まるケースは、`crypto` feature を外した版では動かさない（`regex` も同じ）。
//! 名前が `sqlite` で始まるケースは、`sqlite` feature を付けた版でだけ動かす

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
        // `crypto` feature を外した版では、std:crypto を使うケースは動かさない
        if name.starts_with("crypto") && !cfg!(feature = "crypto") {
            continue;
        }
        // `ui` `audio` feature を付けた版では、あるモジュールの一覧が変わる
        if name == "std_missing.dol"
            && (cfg!(feature = "ui")
                || cfg!(feature = "audio")
                || cfg!(feature = "sqlite")
                || cfg!(feature = "ffi")
                || !cfg!(feature = "image"))
        {
            continue;
        }
        // `image` feature を外した版では、std:image を使うケースは動かさない
        if name.starts_with("image") && !cfg!(feature = "image") {
            continue;
        }
        // `regex` feature を外した版では、std:regex を使うケースは動かさない
        if name.starts_with("regex") && !cfg!(feature = "regex") {
            continue;
        }
        // `sqlite` feature は既定では無効
        if name.starts_with("sqlite") && !cfg!(feature = "sqlite") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let variant = path.with_extension("nohttp.out");
        let expected_path = if !cfg!(feature = "http") && variant.exists() {
            variant
        } else {
            path.with_extension("out")
        };
        let expected = fs::read_to_string(expected_path).unwrap_or_default();

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
