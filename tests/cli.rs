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

#[test]
fn check_reports_errors_without_running() {
    let good = script("check_good.dol", "log[\"実行されない\"]\n");
    let output = dolphin().arg("check").arg(&good).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");

    let bad = script("check_bad.dol", "log[\"x\"]\nundefined_fn[1]\n");
    let output = dolphin()
        .arg("check")
        .arg(&good)
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("未定義の関数 `undefined_fn`"));
    assert_eq!(stdout(&output), "");

    assert_eq!(
        dolphin().arg("check").output().unwrap().status.code(),
        Some(2)
    );
}

#[test]
fn test_runs_test_functions() {
    let dir = std::env::temp_dir().join("dolphin-test-runner");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(
        dir.join("a_test.dol"),
        "@base := 10\nhelper[] ( # 1 )\ntest_add[] (\n    assert_eq[@base + helper[], 11]\n)\ntest_with_arg[@x] (\n    assert[false]\n)\nnot_a_test[] (\n    assert[false]\n)\ntest_log[] (\n    log[\"出力\"]\n)\n",
    )
    .unwrap();
    fs::write(
        dir.join("sub").join("b_test.dol"),
        "test_fail[] (\n    assert_eq[1, 2]\n)\ntest_after[] (\n    assert[true]\n)\nlog[\"本体の出力\"]\n",
    )
    .unwrap();
    // `_test.dol` で終わらないファイルは、ディレクトリを渡したときは見ない
    fs::write(dir.join("other.dol"), "assert[false]\n").unwrap();

    let output = dolphin().arg("test").arg(&dir).output().unwrap();
    let out = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{out}");
    assert!(out.contains("  ok   test_add\n"), "{out}");
    assert!(out.contains("  ok   test_log\n"), "{out}");
    assert!(out.contains("  FAIL test_fail\n"), "{out}");
    assert!(out.contains("  ok   test_after\n"), "{out}");
    assert!(
        !out.contains("test_with_arg") && !out.contains("not_a_test"),
        "{out}"
    );
    assert!(out.contains("assert_eq に失敗しました"), "{out}");
    // 出力は、失敗したファイルのぶんだけ出る
    assert!(
        out.contains("本体の出力") && !out.contains("  出力\n"),
        "{out}"
    );
    assert!(out.ends_with("3 件成功、1 件失敗\n"), "{out}");

    // 成功するファイルだけなら 0
    let output = dolphin()
        .arg("test")
        .arg(dir.join("a_test.dol"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).ends_with("2 件成功、0 件失敗\n"));

    // テストのないディレクトリは失敗
    let empty = dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let output = dolphin().arg("test").arg(&empty).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("テストが見つかりません"));
}
