//! コマンドラインの動作のテスト。`dolphin` の実行ファイルを起動して確かめる
//! （`os.args`、`os.exit`、`os.run`）

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn dolphin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dolphin"))
}

/// テスト用のスクリプトを一時ファイルに書いて、そのパスを返す
fn script(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("dolphin-cli-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, source).unwrap();
    path
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn script_receives_arguments() {
    let path = script(
        "args.dol",
        "@os := import[\"std:os\"]\nlog[@os.args[], arr_len[@os.args[]]]\n",
    );
    let output = dolphin()
        .arg(&path)
        .args(["a", "b c", "--ast"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "{\"a\", \"b c\", \"--ast\"} 3\n");

    let output = dolphin().arg(&path).output().unwrap();
    assert_eq!(stdout(&output), "{} 0\n");
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let output = dolphin().arg("--nope").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("使い方"));
}

#[test]
fn exit_stops_the_script_with_a_code() {
    let path = script(
        "exit.dol",
        r#"@os := import["std:os"]
log["before"]
stop[@code] (
    @os.exit[@code]
    log["not reached"]
)
for @i in range[0, 3] (
    if @i == 1 (
        stop[7]
    )
    log["loop", @i]
)
log["after"]
"#,
    );
    let output = dolphin().arg(&path).output().unwrap();
    assert_eq!(stdout(&output), "before\nloop 0\n");
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(stderr(&output), "");
}

#[test]
fn exit_without_argument_is_success() {
    let path = script(
        "exit0.dol",
        "@os := import[\"std:os\"]\nlog[\"x\"]\n@os.exit[]\nlog[\"y\"]\n",
    );
    let output = dolphin().arg(&path).output().unwrap();
    assert_eq!(stdout(&output), "x\n");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn exit_from_a_callback_and_an_imported_file() {
    let lib = script(
        "exit_lib.dol",
        "@os := import[\"std:os\"]\nlog[\"lib\"]\n@os.exit[5]\n",
    );
    let main = script(
        "exit_main.dol",
        &format!(
            "log[arr_map[{{1, 2}}, [@x] ( # @x * 2 )]]\n@m := import[\"{}\"]\nlog[\"not reached\"]\n",
            lib.file_name().unwrap().to_string_lossy()
        ),
    );
    let output = dolphin().arg(&main).output().unwrap();
    assert_eq!(stdout(&output), "{2, 4}\nlib\n");
    assert_eq!(output.status.code(), Some(5));
}

#[test]
fn runtime_error_exits_with_one() {
    let path = script("fail.dol", "log[1 / 0]\n");
    let output = dolphin().arg(&path).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("0 で割ることはできません"));
}

/// 子プロセスとして動かすスクリプトと、それを起動する親のスクリプトを作る
fn run_in_child(tag: &str, child_source: &str, parent_body: &str) -> Output {
    // テストは並行して動くので、ファイル名はテストごとに変える
    let child = script(&format!("child_{tag}.dol"), child_source);
    let parent = script(
        &format!("parent_{tag}.dol"),
        &format!(
            "@os := import[\"std:os\"]\n@exe := @os.args[]{{0}}\n@child := @os.args[]{{1}}\n{parent_body}"
        ),
    );
    dolphin()
        .arg(&parent)
        .arg(env!("CARGO_BIN_EXE_dolphin"))
        .arg(&child)
        .output()
        .unwrap()
}

#[test]
fn run_captures_output_and_status() {
    let output = run_in_child(
        "capture",
        "@os := import[\"std:os\"]\nlog[\"hello \" + input[]]\nlog[\"args:\", @os.args[]]\n@os.exit[3]\n",
        r#"
@r := @os.run[@exe, {@child, "x", "y z"}, {"stdin": "bob\n"}]?
log[@r{"status"}, @r{"ok"}, @r{"stdout"}, @r{"stderr"}]
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "3 false hello bob\nargs: {\"x\", \"y z\"}\n \n"
    );
}

#[test]
fn run_options_env_and_cwd() {
    let dir = std::env::temp_dir().join("dolphin-cli-tests");
    let output = run_in_child(
        "env_cwd",
        "@os := import[\"std:os\"]\nlog[@os.env[\"DOL_TEST_VAR\"]]\nlog[@os.cwd[]]\n",
        &format!(
            r#"
@r := @os.run[@exe, {{@child}}, {{"env": {{"DOL_TEST_VAR": "from parent"}}, "cwd": "{}"}}]?
log[@r{{"ok"}}]
@lines := str_lines[@r{{"stdout"}}]
log[@lines{{0}}, str_ends_with[@lines{{1}}, "dolphin-cli-tests"]]
"#,
            dir.to_string_lossy().replace('\\', "/")
        ),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "true\nfrom parent true\n");
}

#[test]
fn run_failures_are_error_values() {
    let output = run_in_child(
        "failures",
        "@t := import[\"std:time\"]\n@t.sleep[10]\n",
        r#"
@missing := @os.run["dolphin-no-such-command-xyz", {}]
log[is_error[@missing], err_kind[@missing]]
@started := import["std:time"].monotonic[]
@slow := @os.run[@exe, {@child}, {"timeout": 0.3}]
log[is_error[@slow], err_kind[@slow], import["std:time"].monotonic[] - @started < 5]
@badcwd := @os.run[@exe, {@child}, {"cwd": "no/such/dir"}]
log[err_kind[@badcwd]]
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "true not_found\ntrue timeout true\nnot_found\n"
    );
}

#[test]
fn run_with_binary_stdin_and_output() {
    let output = run_in_child(
        "binary",
        "log[\"got \" + input[]]
",
        r#"
@r := @os.run[@exe, {@child}, {"stdin": bytes_from_arr[{104, 105, 10}], "as": "bytes"}]?
log[type_of[@r{"stdout"}], bytes_to_str[@r{"stdout"}], type_of[@r{"stderr"}], bytes_len[@r{"stderr"}]]
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "bytes got hi
 bytes 0
"
    );
}
