//! `os.spawn` のテスト（design.md 5.42）。子プロセスには、dolphin 自身を使う

use std::fs;
use std::path::PathBuf;

const DOLPHIN: &str = env!("CARGO_BIN_EXE_dolphin");

/// 子プロセスに動かすスクリプトを書いて、パスを返す
fn child_script(name: &str, source: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(name);
    fs::write(&path, source).unwrap();
    path.to_string_lossy().replace('\\', "/")
}

/// スクリプトを実行して出力を返す。`@exe` に dolphin のパスを入れておく
fn run(script: &str) -> String {
    let src = format!(
        "@os := import[\"std:os\"]\n@s := import[\"std:stream\"]\n@exe := \"{}\"\n{script}",
        DOLPHIN.replace('\\', "/")
    );
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

#[test]
fn talk_to_a_resident_process() {
    let upper = child_script(
        "spawn_upper.dol",
        r#"@s := import["std:stream"]
@s.each_line[@s.stdin[], [@l] ( @s.write[@s.stdout[], str_upper[@l] + "\n"]? )]?
"#,
    );
    let out = run(&format!(
        r#"
@p := @os.spawn[@exe, {{"{upper}"}}]?
log[type_of[@p{{"process"}}], @p{{"stderr"}}, type_of[@p{{"pid"}}]]
@s.write[@p{{"stdin"}}, "hello\n"]?
log[@s.read_line[@p{{"stdout"}}]?]
@s.write[@p{{"stdin"}}, "dolphin\n"]?
log[@s.read_line[@p{{"stdout"}}]?]
log[@os.poll[@p{{"process"}}]]
@s.close[@p{{"stdin"}}]
log[@s.read_line[@p{{"stdout"}}]?]
log[@os.wait[@p{{"process"}}]]
log[@os.wait[@p{{"process"}}], @os.poll[@p{{"process"}}]]
@os.kill[@p{{"process"}}]
"#
    ));
    assert_eq!(
        out,
        "handle nil number\nHELLO\nDOLPHIN\nnil\nnil\n{\"status\": 0, \"ok\": true}\n{\"status\": 0, \"ok\": true} {\"status\": 0, \"ok\": true}\n"
    );
}

#[test]
fn exit_code_timeout_and_kill() {
    let exit3 = child_script(
        "spawn_exit3.dol",
        "@os := import[\"std:os\"]\n@os.exit[3]\n",
    );
    let sleeper = child_script(
        "spawn_sleep.dol",
        "@t := import[\"std:time\"]\n@t.sleep[60]\n",
    );
    let out = run(&format!(
        r#"
@a := @os.spawn[@exe, {{"{exit3}"}}, {{"stdin": "null", "stdout": "null"}}]?
log[@a{{"stdin"}}, @a{{"stdout"}}, @a{{"stderr"}}]
log[@os.wait[@a{{"process"}}]]

@b := @os.spawn[@exe, {{"{sleeper}"}}, {{"stdin": "null", "stdout": "null"}}]?
log[@os.wait[@b{{"process"}}, 0.2]]
log[@os.poll[@b{{"process"}}]]
@os.kill[@b{{"process"}}]
log[@os.poll[@b{{"process"}}]{{"ok"}}]
log[@os.wait[@b{{"process"}}]{{"ok"}}]
"#
    ));
    assert_eq!(
        out,
        "nil nil nil\n{\"status\": 3, \"ok\": false}\nnil\nnil\nfalse\nfalse\n"
    );
}

#[test]
fn options_env_cwd_and_errors() {
    let show = child_script(
        "spawn_env.dol",
        r#"@os := import["std:os"]
@s := import["std:stream"]
@s.write[@s.stdout[], to_str[@os.env["SPAWN_TEST"]] + " " + @os.cwd[]]
"#,
    );
    let out = run(&format!(
        r#"
@dir := @os.temp_dir[]
@p := @os.spawn[@exe, {{"{show}"}}, {{"env": {{"SPAWN_TEST": "yes"}}, "cwd": @dir, "stdin": "null"}}]?
@t := @s.read_text[@p{{"stdout"}}]?
log[str_starts_with[@t, "yes "], @t == "yes " + @os.cwd[]]
@q := @os.spawn[@exe, {{"{show}"}}, {{"stdin": "null"}}]?
log[str_starts_with[@s.read_text[@q{{"stdout"}}]?, "nil "]]
"#
    ));
    // 子プロセスの cwd は `temp_dir` なので、親の cwd とは違う
    assert_eq!(
        out,
        "true false
true
"
    );

    let out = run(r#"
log[err_kind[@os.spawn["definitely_not_a_command_xyz", {}]]]
log[err_kind[@os.spawn[@exe, {}, {"cwd": "target/no_such_dir_xyz"}]]]
"#);
    assert_eq!(
        out,
        "not_found
not_found
"
    );
}

#[test]
fn argument_mistakes() {
    let message = |script: &str| {
        let src = format!("@os := import[\"std:os\"]\n@s := import[\"std:stream\"]\n{script}");
        let mut out = Vec::new();
        dolphin::run_source(&src, &mut out).unwrap_err().message
    };
    assert!(message(r#"@os.spawn["x", {}, {"nope": 1}]"#).contains("知らないオプション `nope`"));
    assert!(message(r#"@os.spawn["x", {}, {"stdout": "pipes"}]"#).contains("\"pipe\""));
    assert!(message(r#"@os.spawn["x", {1}]"#).contains("文字列"));
    assert!(message(r#"@os.wait[1]"#).contains("プロセス"));
    assert!(message(r#"@os.wait[@s.stdout[]]"#).contains("os.spawn が返したプロセス"));
}
