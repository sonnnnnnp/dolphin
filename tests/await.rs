//! `async` と `await` のテスト（design.md 5.46）

/// 本体と同じ大きさのスタックのスレッドで動かす（`async` 関数のその場での入れ子は Rust の再帰を使う）
fn big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn run(script: &str) -> String {
    let script = script.to_string();
    big_stack(move || {
        let mut out = Vec::new();
        if let Err(diag) = dolphin::run_source(&script, &mut out) {
            panic!(
                "{}
{}",
                diag.render("test.dol", &script),
                String::from_utf8_lossy(&out)
            );
        }
        String::from_utf8(out).unwrap()
    })
}

/// 実行時エラーか実行前のエラーで止まったときの表示
fn run_err(script: &str) -> String {
    let script = script.to_string();
    big_stack(move || {
        let mut out = Vec::new();
        match dolphin::run_source(&script, &mut out) {
            Err(diag) => diag.render("test.dol", &script),
            Ok(()) => panic!("エラーになるはず: {}", String::from_utf8_lossy(&out)),
        }
    })
}

#[test]
fn async_function_starts_eagerly_and_returns_a_future() {
    let out = run(r#"
async add[@a, @b] (
    log["add の中"]
    # @a + @b
)
log["呼ぶ前"]
@f := add[1, 2]
log["呼んだ後", type_of[@f]]
log[await @f]
log[@f]
"#);
    assert_eq!(out, "呼ぶ前\nadd の中\n呼んだ後 future\n3\n<future 完了>\n");
}

#[test]
fn await_suspends_and_other_flows_run() {
    let out = run(r#"
@async := import["std:async"]
async slow[@name, @secs] (
    log[@name, "開始"]
    await @async.sleep[@secs]
    log[@name, "終了"]
    # @name
)
async main[] (
    @a := slow["a", 0.06]
    @b := slow["b", 0.02]
    log["main は待つ"]
    @all := await @async.all[{@a, @b}]
    log[@all]
)
await main[]
log["おわり"]
"#);
    assert_eq!(
        out,
        "a 開始\nb 開始\nmain は待つ\nb 終了\na 終了\n{\"a\", \"b\"}\nおわり\n"
    );
}

#[test]
fn sequential_awaits_in_a_loop() {
    let out = run(r#"
@async := import["std:async"]
async count[@n] (
    @total := 0
    for @i in range[0, @n] (
        await @async.sleep[0.001]
        @total = @total + @i
    )
    # @total
)
log[await count[5]]
@xs := arr_map[{1, 2, 3}, [@n] ( # count[@n] )]
log[type_of[@xs{0}]]
log[await @async.all[@xs]]
"#);
    assert_eq!(out, "10\nfuture\n{0, 1, 3}\n");
}

#[test]
fn async_lambda_and_member_function() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("await_mod.dol"),
        "@async := import[\"std:async\"]\nasync twice[@x] (\n    await @async.sleep[0.001]\n    # @x * 2\n)\n",
    )
    .unwrap();
    let src = r#"
@m := import["await_mod.dol"]
log[await @m.twice[21]]
@f := async [@x] ( # await @m.twice[@x] + 1 )
log[await @f[1]]
@g := @m.twice
log[await @g[5]]
"#;
    let mut out = Vec::new();
    dolphin::run_source_in(
        src,
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        &mut out,
    )
    .unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "42\n3\n10\n");
}

#[test]
fn errors_fail_the_future_and_are_raised_by_await() {
    let out = run(r#"
async boom[] (
    # 1 / 0
)
async inner[] (
    # await boom[]
)
async outer[] (
    @f := inner[]
    log["未了でも outer は進む"]
    # await @f
)
@f := outer[]
log[type_of[@f]]
"#);
    assert_eq!(out, "未了でも outer は進む\nfuture\n");

    // 最上位の await で、失敗がそのまま実行時エラーになる
    let message = run_err(
        r#"
async boom[] (
    # 1 / 0
)
await boom[]
"#,
    );
    assert!(message.contains("0 で割ることはできません"), "{message}");
    assert!(message.contains("boom の中"), "{message}");
}

#[test]
fn try_applies_to_the_awaited_value() {
    let out = run(r#"
async fails[] ( # err_new["bad", "m"] )
async pass[] (
    @v := await fails[]?
    # "ここには来ない"
)
@r := await pass[]
log[is_error[@r], err_kind[@r]]
"#);
    assert_eq!(out, "true bad\n");
}

#[test]
fn where_await_is_allowed() {
    // 普通の関数の中は、実行前のエラー
    let message = run_err(
        r#"
async f[] ( # 1 )
g[] (
    # await f[]
)
"#,
    );
    assert!(
        message.contains("`await` は `async` 関数の中か"),
        "{message}"
    );
    // async でない無名関数の中も同じ
    let message = run_err(
        r#"
async f[] ( # 1 )
async h[] (
    @x := [] ( # await f[] )
)
"#,
    );
    assert!(
        message.contains("`await` は `async` 関数の中か"),
        "{message}"
    );
}

#[test]
fn await_needs_a_future() {
    let message = run_err("await 5");
    assert!(
        message.contains("await できるのは future だけです（数値でした）"),
        "{message}"
    );
    let message = run_err("async f[] ( await \"x\" )\nawait f[]");
    assert!(
        message.contains("future だけです（文字列でした）"),
        "{message}"
    );
}

#[test]
fn async_keyword_mistakes_are_syntax_errors() {
    assert!(run_err("async 1").contains("`async` の無名関数"));
    assert!(run_err("async f ( # 1 )").contains("async"));
}

#[test]
fn nothing_to_wait_for_is_an_error() {
    // 一度止まってから、自分自身の future を待つ流れ。待つものが何もなくなる
    let message = run_err(
        r#"
@async := import["std:async"]
@box := {nil}
async wait_self[] (
    await @async.sleep[0.001]
    # await @box{0}
)
@box{0} = wait_self[]
await @box{0}
"#,
    );
    assert!(message.contains("終わる見込みがありません"), "{message}");
}

#[test]
fn depth_limit_applies_per_flow() {
    let message = run_err(
        r#"
async deep[@n] (
    # await deep[@n + 1]
)
await deep[0]
"#,
    );
    assert!(message.contains("関数の呼び出しが深すぎます"), "{message}");
}
