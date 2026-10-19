//! `std:stream` が `std:net` の TCP 接続にも使えること（design.md 5.41）

fn run(script: &str) -> String {
    let mut out = Vec::new();
    if let Err(diag) = dolphin::run_source(script, &mut out) {
        panic!(
            "{}\n{}",
            diag.render("test.dol", script),
            String::from_utf8_lossy(&out)
        );
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn tcp_connection_as_a_stream() {
    let out = run(r#"
@net := import["std:net"]
@s := import["std:stream"]
@l := @net.listen[0]?
@port := @net.local_addr[@l]{"port"}
@client := @net.connect["127.0.0.1", @port]?
@server := @net.accept[@l]?
@s.write[@client, "hello\nworld\n"]?
@s.write[@client, str_to_bytes["tail"]]?
@s.flush[@client]?
log[@s.read_line[@server]?, @s.read_exact[@server, 5]?]
@s.close[@client]?
log[@s.read_text[@server]?]
log[@s.read_line[@server]?]
@s.close[@server]
log[err_kind[@s.read_line[@client]], err_kind[@net.read[@server, 1]]]
"#);
    assert_eq!(
        out,
        "hello <bytes 776f726c64>\n\ntail\nnil\nclosed closed\n"
    );
}
