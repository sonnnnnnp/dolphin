//! `std:worker` のテスト（design.md 5.43）

use std::fs;
use std::path::PathBuf;

/// ワーカーのスクリプトを置くディレクトリ（テストごとに別の名前のファイルを使う）
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

fn write_script(name: &str, source: &str) {
    fs::write(dir().join(name), source).unwrap();
}

/// 親のスクリプトを `dir()` を基準に実行して、出力を返す
fn run(script: &str) -> String {
    let src = format!("@w := import[\"std:worker\"]\n{script}");
    let mut out = Vec::new();
    if let Err(diag) = dolphin::run_source_in(&src, dir(), &mut out) {
        panic!(
            "{}\n{}",
            diag.render("test.dol", &src),
            String::from_utf8_lossy(&out)
        );
    }
    String::from_utf8(out).unwrap()
}

fn run_err(script: &str) -> String {
    let src = format!("@w := import[\"std:worker\"]\n{script}");
    let mut out = Vec::new();
    dolphin::run_source_in(&src, dir(), &mut out)
        .unwrap_err()
        .message
}

#[test]
fn blocking_worker_computes_in_parallel() {
    write_script(
        "worker_fib.dol",
        r#"@w := import["std:worker"]
fib[@n] (
    if @n < 2 ( # @n )
    # fib[@n - 1] + fib[@n - 2]
)
@m := @w.recv[]
while @m != nil (
    @w.send[{"n": @m{"n"}, "fib": fib[@m{"n"}]}]
    @m = @w.recv[]
)
"#,
    );
    let out = run(r#"
// 4 つのワーカーに同時に頼む
@jobs := {}
for @n in {15, 16, 17, 18} (
    @job := @w.spawn["worker_fib.dol"]?
    @w.send[@job, {"n": @n}]?
    arr_push[@jobs, @job]
)
for @job in @jobs (
    @r := @w.recv[@job]?
    log[@r{"n"}, @r{"fib"}]
    @w.terminate[@job]
)
"#);
    assert_eq!(out, "15 610\n16 987\n17 1597\n18 2584\n");
}

#[test]
fn arg_exit_status_and_join() {
    write_script(
        "worker_arg.dol",
        r#"@w := import["std:worker"]
@os := import["std:os"]
@w.send[{"arg": @w.arg[], "is_worker": @w.is_worker[]}]
@w.send[bytes_from_arr[{1, 2}]]
@w.send[1234567890123456789i]
@os.exit[@w.arg[]{"code"}]
"#,
    );
    let out = run(r#"
log[@w.is_worker[]]
@job := @w.spawn["worker_arg.dol", {"arg": {"code": 3, "list": {1, "two", nil}}}]?
log[type_of[@job], @job]
log[@w.recv[@job]]
log[@w.recv[@job]]
log[@w.recv[@job], @w.recv[@job]]
log[@w.join[@job]]
log[@w.join[@job]{"status"}]
"#);
    assert_eq!(
        out,
        "false
handle <worker worker_arg.dol>
{\"arg\": {\"code\": 3, \"list\": {1, \"two\", nil}}, \"is_worker\": true}
<bytes 0102>
1234567890123456789 nil
{\"ok\": false, \"status\": 3, \"error\": nil}
3
"
    );
}

#[test]
fn errors_in_the_worker_are_reported_by_join() {
    write_script("worker_boom.dol", "@x := 1 + \"a\"\n");
    write_script("worker_syntax.dol", "@x := (\n");
    let out = run(r#"
@a := @w.spawn["worker_boom.dol"]?
@r := @w.join[@a]
log[@r{"ok"}, @r{"status"}, str_contains[@r{"error"}, "worker_boom.dol"]]
@b := @w.spawn["worker_syntax.dol"]?
@s := @w.join[@b]
log[@s{"ok"}, str_contains[@s{"error"}, "worker_syntax.dol"]]
log[@w.recv[@b]]
"#);
    assert_eq!(out, "false 1 true\nfalse true\nnil\n");
}

#[test]
fn callback_worker_uses_the_event_loop() {
    write_script(
        "worker_count.dol",
        r#"@w := import["std:worker"]
for @i in range[0, @w.arg[]] (
    @w.send[@i * 10]
)
"#,
    );
    let out = run(r#"
@got := {}
@w.spawn["worker_count.dol", {
    "arg": 3,
    "on_message": [@worker, @data] ( arr_push[@got, @data] ),
    "on_close": [@worker, @status, @error] (
        log[@got, @status, @error == "", @worker]
    )
}]?
log["待たずに進む"]
"#);
    assert_eq!(
        out,
        "待たずに進む\n{0, 10, 20} 0 true <worker 閉じています>\n"
    );
}

#[test]
fn callback_worker_error_is_passed_to_on_close() {
    write_script(
        "worker_cb_boom.dol",
        "@w := import[\"std:worker\"]\n@w.nope[]\n",
    );
    let out = run(r#"
@w.spawn["worker_cb_boom.dol", {"on_close": [@worker, @status, @error] (
    log[@status, str_contains[@error, "worker_cb_boom.dol"]]
)}]?
"#);
    assert_eq!(out, "1 true\n");
}

#[test]
fn terminate_and_closed_workers() {
    write_script(
        "worker_echo.dol",
        r#"@w := import["std:worker"]
@m := @w.recv[]
while @m != nil (
    @w.send[@m]
    @m = @w.recv[]
)
"#,
    );
    let out = run(r#"
@job := @w.spawn["worker_echo.dol"]?
@w.send[@job, "ping"]?
log[@w.recv[@job]]
log[err_kind[@w.recv[@job, 0.05]]]
log[@w.terminate[@job]]
log[err_kind[@w.send[@job, 1]], err_kind[@w.recv[@job]], @job]
log[@w.terminate[@job]]
"#);
    assert_eq!(
        out,
        "ping\ntimeout\nnil\nclosed closed <worker 閉じています>\nnil\n"
    );
}

#[test]
fn spawn_failures_and_mistakes() {
    let out = run(r#"log[err_kind[@w.spawn["worker_no_such_file.dol"]]]"#);
    assert_eq!(out, "not_found\n");
    assert!(run_err(r#"@w.spawn["x.dol", {"nope": 1}]"#).contains("知らないオプション `nope`"));
    assert!(run_err(r#"@w.spawn["x.dol", {"on_message": 1}]"#).contains("`on_message` は関数です"));
    assert!(run_err("@w.recv[]").contains("ワーカーの中でだけ"));
    assert!(run_err("@w.arg[]").contains("ワーカーの中でだけ"));
    assert!(run_err("@w.send[1]").contains("ワーカーの中でだけ"));
    assert!(run_err("@w.recv[1]").contains("ワーカー"));
    assert!(run_err(r#"@w.recv[nil]"#).contains("ワーカー"));
}

#[test]
fn recv_on_a_callback_worker_is_a_mistake() {
    write_script("worker_idle.dol", "");
    let message = run_err(
        r#"
@job := @w.spawn["worker_idle.dol", {"on_close": [@a, @b, @c] ( log[1] )}]?
@w.recv[@job]
"#,
    );
    assert!(message.contains("コールバック型"), "{message}");
}
