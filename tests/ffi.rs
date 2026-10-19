//! `std:ffi` のテスト（design.md 5.44）。OS の C ランタイムの関数を呼ぶ

#![cfg(feature = "ffi")]

/// C の標準関数（strlen・memset など）のあるライブラリ
const LIBC: &str = if cfg!(windows) {
    "ucrtbase.dll"
} else if cfg!(target_os = "macos") {
    "libSystem.B.dylib"
} else {
    "libc.so.6"
};

/// 数学関数（pow）のあるライブラリ
const LIBM: &str = if cfg!(windows) {
    "ucrtbase.dll"
} else if cfg!(target_os = "macos") {
    "libSystem.B.dylib"
} else {
    "libm.so.6"
};

fn script(body: &str) -> String {
    format!(
        "@ffi := import[\"std:ffi\"]\n@c := @ffi.open[\"{LIBC}\"]?\n@m := @ffi.open[\"{LIBM}\"]?\n{body}"
    )
}

fn run(body: &str) -> String {
    let src = script(body);
    let mut out = Vec::new();
    if let Err(diag) = dolphin::run_source(&src, &mut out) {
        panic!(
            "{}\n{}",
            diag.render("test.dol", &src),
            String::from_utf8_lossy(&out)
        );
    }
    String::from_utf8(out).unwrap()
}

fn run_err(body: &str) -> String {
    let src = script(body);
    let mut out = Vec::new();
    dolphin::run_source(&src, &mut out).unwrap_err().message
}

#[test]
fn numbers_in_and_out() {
    let out = run(r#"
@pow := @ffi.func[@m, "pow", {"f64", "f64"}, "f64"]?
log[@ffi.call[@pow, {2, 10}], @ffi.call[@pow, {2.5, 2}]]
@abs := @ffi.func[@c, "abs", {"i32"}, "i32"]?
log[@ffi.call[@abs, {-5}], @ffi.call[@abs, {7i}]]
@llabs := @ffi.func[@c, "llabs", {"i64"}, "i64"]?
@big := @ffi.call[@llabs, {-1234567890123456789i}]
log[type_of[@big], @big]
@toupper := @ffi.func[@c, "toupper", {"i32"}, "i32"]?
log[@ffi.call[@toupper, {97}]]
log[type_of[@pow], @pow]
"#);
    assert_eq!(
        out,
        "1024 6.25\n5 7\nint 1234567890123456789\n65\nhandle <ffi 関数 pow>\n"
    );
}

#[test]
fn strings_buffers_and_pointers() {
    let out = run(r#"
@strlen := @ffi.func[@c, "strlen", {"str"}, "u64"]?
@n := @ffi.call[@strlen, {"イルカ"}]
log[type_of[@n], @n]

@buf := @ffi.buffer[8]
log[@buf, @ffi.size[@buf], @ffi.read[@buf, 0, 8]]
@memset := @ffi.func[@c, "memset", {"buf", "i32", "u64"}, "ptr"]?
@p := @ffi.call[@memset, {@buf, 65, 4}]
log[@p == @ffi.address[@buf], @ffi.read[@buf, 0, 6]]

@memcpy := @ffi.func[@c, "memcpy", {"buf", "bytes", "u64"}, "ptr"]?
@ffi.call[@memcpy, {@buf, str_to_bytes["hey"], 3}]
log[@ffi.read[@buf, 0, 4]]
@ffi.write[@buf, 4, str_to_bytes["!?"]]
log[@ffi.read[@buf, 3, 4]]
@copy := @ffi.buffer_from[str_to_bytes["abc"]]
log[@ffi.read[@copy, 1, 2], @ffi.size[@copy]]

// アドレスを返す関数と、アドレスから読む関数
@strcpy := @ffi.func[@c, "strcpy", {"buf", "str"}, "ptr"]?
@z := @ffi.buffer[16]
@addr := @ffi.call[@strcpy, {@z, "dolphin"}]
log[type_of[@addr], @ffi.cstr[@addr], @ffi.peek[@addr, 3]]
@strchr := @ffi.func[@c, "strchr", {"str", "i32"}, "str"]?
log[@ffi.call[@strchr, {"hello", 108}], @ffi.call[@strchr, {"hello", 122}]]
@nullcheck := @ffi.func[@c, "strchr", {"ptr", "i32"}, "ptr"]?
log[@ffi.call[@nullcheck, {@addr, 122}]]
"#);
    assert_eq!(
        out,
        "int 9
<ffi バッファ 8 バイト> 8 <bytes 0000000000000000>
true <bytes 414141410000>
<bytes 68657941>
<bytes 41213f00>
<bytes 6263> 3
int dolphin <bytes 646f6c>
llo nil
nil
"
    );
}

#[test]
fn failures_are_error_values() {
    let out = run(r#"
log[err_kind[@ffi.open["definitely_not_a_library_xyz"]]]
log[err_kind[@ffi.func[@c, "definitely_not_a_symbol_xyz", {}, "void"]]]
"#);
    assert_eq!(out, "not_found\nnot_found\n");
}

#[test]
fn mistakes_are_runtime_errors() {
    let prefix = r#"@abs := @ffi.func[@c, "abs", {"i32"}, "i32"]?
@b := @ffi.buffer[4]
"#;
    let cases: &[(&str, &str)] = &[
        (r#"@ffi.func[@c, "abs", {"int"}, "i32"]"#, "知りません"),
        (
            r#"@ffi.func[@c, "abs", {"void"}, "i32"]"#,
            "`void` は使えません",
        ),
        (r#"@ffi.func[@c, "abs", {"i32"}, "buf"]"#, "引数だけ"),
        ("@ffi.call[@abs, {}]", "引数の配列は 1 個です"),
        ("@ffi.call[@abs, {1, 2}]", "引数の配列は 1 個です"),
        (r#"@ffi.call[@abs, {"a"}]"#, "数値か整数です"),
        ("@ffi.call[@abs, {1.5}]", "整数の値です"),
        ("@ffi.call[@abs, {3000000000}]", "i32 の範囲を出ています"),
        ("@ffi.call[@b, {1}]", "関数です"),
        ("@ffi.read[@abs, 0, 1]", "バッファです"),
        ("@ffi.read[@b, 3, 2]", "大きさ 4 を出ています"),
        ("@ffi.read[@b, -1, 1]", "を出ています"),
        (r#"@ffi.write[@b, 3, str_to_bytes["ab"]]"#, "を出ています"),
        ("@ffi.buffer[-1]", "0〜"),
        ("@ffi.peek[0, 1]", "NULL"),
        ("@ffi.size[1]", "バッファです"),
    ];
    for (body, expected) in cases {
        let message = run_err(&format!("{prefix}{body}"));
        assert!(message.contains(expected), "{body}: {message}");
    }
    let strlen = r#"@strlen := @ffi.func[@c, "strlen", {"str"}, "u64"]?
@ffi.call[@strlen, {"a" + str_from_code[0]}]"#;
    assert!(run_err(strlen).contains("NUL"));
}
