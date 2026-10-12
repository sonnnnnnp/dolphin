//! 命令の組み立て・入力の関数・フレームの往復のテスト（描画は手で確かめる）

use super::*;
use crate::syntax::{lexer, parser};

fn eval(interp: &mut Interpreter<'_>, src: &str) -> Result<Value, String> {
    let tokens = lexer::tokenize(src).map_err(|e| e.message)?;
    let program = parser::parse(&tokens).map_err(|e| e.message)?;
    interp
        .run(&program, true)
        .map(|v| v.unwrap_or(Value::Nil))
        .map_err(|e| e.message)
}

fn with_canvas(test: impl FnOnce(&mut Interpreter<'_>)) {
    let mut out = Vec::new();
    let mut interp = Interpreter::new(&mut out, ".".into());
    crate::stdlib::register(&mut interp);
    super::register(&mut interp);
    test(&mut interp);
}

/// `ui.run` の準備だけを、GUI なしで再現する
fn start(interp: &mut Interpreter<'_>, draw: &str, update: Option<&str>) -> Receiver<Vec<Cmd>> {
    let draw = eval(interp, draw).unwrap();
    let update = update.map(|u| eval(interp, u).unwrap());
    let (frames, rx) = mpsc::channel();
    STATE.with(|state| {
        *state.borrow_mut() = Some(State {
            update,
            draw,
            frames,
            repaint: Arc::new(Mutex::new(None)),
            quit: Arc::new(AtomicBool::new(false)),
            width: 320.0,
            height: 200.0,
            input: Input::default(),
            cmds: None,
            last_error: None,
        });
    });
    rx
}

fn stop() {
    STATE.with(|state| *state.borrow_mut() = None);
}

#[test]
fn draw_builds_commands_in_order() {
    with_canvas(|interp| {
        eval(interp, "@c := import[\"std:canvas\"]").unwrap();
        let rx = start(
            interp,
            r##"[] (
    @c.clear["#102030"]
    @c.rect[1, 2, 30, 40, "#ff0000"]
    @c.rect[1, 2, 3, 4, "#00ff0080", {"stroke": 2}]
    @c.circle[5, 6, 7, "#ffffff"]
    @c.circle[5, 6, 7, "#ffffff", {"stroke": 1.5}]
    @c.line[0, 0, 10, 10, "#000000"]
    @c.line[0, 0, 10, 10, "#000000", 3]
    @c.polygon[{{0, 0}, {10, 0}, {5, 8}}, "#123456"]
    @c.text[1, 2, 42, "#abcdef"]
    @c.text[1, 2, "こんにちは", "#abcdef", 20]
)"##,
            None,
        );
        frame(interp, 0.016, Input::default()).unwrap();
        let cmds = rx.try_recv().unwrap();
        assert_eq!(cmds.len(), 10);
        assert_eq!(cmds[0], Cmd::Clear([0x10, 0x20, 0x30, 255]));
        assert_eq!(
            cmds[1],
            Cmd::Rect {
                x: 1.0,
                y: 2.0,
                w: 30.0,
                h: 40.0,
                color: [255, 0, 0, 255],
                stroke: None
            }
        );
        assert!(matches!(
            cmds[2],
            Cmd::Rect {
                color: [0, 255, 0, 128],
                stroke: Some(s),
                ..
            } if s == 2.0
        ));
        assert!(matches!(cmds[4], Cmd::Circle { stroke: Some(s), .. } if s == 1.5));
        assert!(matches!(cmds[5], Cmd::Line { width, .. } if width == 1.0));
        assert!(matches!(cmds[6], Cmd::Line { width, .. } if width == 3.0));
        assert_eq!(
            cmds[7],
            Cmd::Polygon {
                points: vec![(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)],
                color: [0x12, 0x34, 0x56, 255],
                stroke: None
            }
        );
        assert!(matches!(&cmds[8], Cmd::Text { text, size, .. } if text == "42" && *size == 14.0));
        assert!(
            matches!(&cmds[9], Cmd::Text { text, size, .. } if text == "こんにちは" && *size == 20.0)
        );

        // 次のフレームは、何もない状態から始まる
        frame(interp, 0.016, Input::default()).unwrap();
        assert_eq!(rx.try_recv().unwrap().len(), 10);
        stop();
    });
}

#[test]
fn update_sees_dt_and_input() {
    with_canvas(|interp| {
        eval(
            interp,
            "@c := import[\"std:canvas\"]\n@dt := 0\n@seen := {}\n@size := nil",
        )
        .unwrap();
        let rx = start(
            interp,
            "[] ( @c.rect[0, 0, 1, 1, \"#ffffff\"] )",
            Some(
                r##"[@d] (
    @dt = @d
    @seen = {
        @c.key_down["left"], @c.key_down["a"], @c.key_pressed["space"],
        @c.key_released["escape"], @c.mouse(){"x"}
    }
    @size = @c.size[]
)"##
                .replace("@c.mouse(){\"x\"}", "@c.mouse[]{\"x\"}")
                .as_str(),
            ),
        );
        let mut input = Input::default();
        input.down.insert("left".into());
        input.pressed.insert("space".into());
        input.released.insert("escape".into());
        input.mouse_x = 12.5;
        input.mouse_y = 3.0;
        input.inside = true;
        input.buttons = [true, false, false];
        input.buttons_pressed = [false, true, false];
        input.wheel = -2.0;
        frame(interp, 0.5, input).unwrap();
        assert!(rx.try_recv().is_ok());
        assert_eq!(eval(interp, "@dt").unwrap().to_string(), "0.5");
        assert_eq!(
            eval(interp, "@seen").unwrap().to_string(),
            "{true, false, true, true, 12.5}"
        );
        assert_eq!(
            eval(interp, "@size").unwrap().to_string(),
            "{\"width\": 320, \"height\": 200}"
        );
        assert_eq!(
            eval(interp, "@c.mouse[]").unwrap().to_string(),
            "{\"x\": 12.5, \"y\": 3, \"inside\": true, \"left\": true, \"right\": false, \"middle\": false, \"left_pressed\": false, \"right_pressed\": true, \"middle_pressed\": false, \"wheel\": -2}"
        );
        stop();
    });
}

#[test]
fn argument_and_context_errors() {
    with_canvas(|interp| {
        eval(interp, "@c := import[\"std:canvas\"]").unwrap();
        // `canvas.run` の外・`draw` の外
        for src in [
            "@c.key_down[\"a\"]",
            "@c.mouse[]",
            "@c.size[]",
            "@c.rect[0, 0, 1, 1, \"#ffffff\"]",
        ] {
            let err = eval(interp, src).unwrap_err();
            assert!(err.contains("だけ使えます"), "{src}: {err}");
        }
        let rx = start(interp, "[] ( 1 )", None);
        // `canvas.run` の中でも、`draw` の外では描けない
        let err = eval(interp, "@c.circle[0, 0, 1, \"#ffffff\"]").unwrap_err();
        assert!(err.contains("`draw` の中でだけ"), "{err}");
        drop(rx);
        stop();

        // 命令の検査（`draw` の中を再現するために、命令の入れ物を用意する）
        let _rx = start(interp, "[] ( 1 )", None);
        STATE.with(|state| state.borrow_mut().as_mut().unwrap().cmds = Some(Vec::new()));
        let cases = [
            ("@c.rect[0, 0, 1, 1]", "引数は 5 個から 6 個"),
            ("@c.rect[0, 0, 1, 1, \"red\"]", "`#rrggbb`"),
            (
                "@c.rect[0, 0, 1, 1, \"#ffffff\", {\"fill\": 1}]",
                "知りません",
            ),
            (
                "@c.rect[0, 0, 1, 1, \"#ffffff\", {\"stroke\": 0}]",
                "0 より大きい",
            ),
            ("@c.rect[\"x\", 0, 1, 1, \"#ffffff\"]", "数値"),
            ("@c.polygon[{{0, 0}, {1, 1}}, \"#ffffff\"]", "3 つ以上"),
            ("@c.polygon[{{0, 0}, {1, 1}, 5}, \"#ffffff\"]", "`{x, y}`"),
            ("@c.text[0, 0, \"x\", \"#ffffff\", 0]", "0 より大きい"),
            ("@c.key_down[\"enterr\"]", "知らないキー"),
            ("@c.key_down[1]", "文字列"),
        ];
        for (src, expected) in cases {
            let err = eval(interp, src).unwrap_err();
            assert!(err.contains(expected), "{src}: {err}");
        }
        stop();
    });
}

#[test]
fn failed_draw_keeps_the_previous_frame() {
    with_canvas(|interp| {
        eval(interp, "@c := import[\"std:canvas\"]\n@fail := false").unwrap();
        let rx = start(
            interp,
            "[] (\n    @c.clear[\"#000000\"]\n    if @fail ( @c.rect[0, 0, 1, 1, \"bad\"] )\n)",
            None,
        );
        frame(interp, 0.016, Input::default()).unwrap();
        assert!(rx.try_recv().is_ok());
        eval(interp, "@fail = true").unwrap();
        // エラーは報告するだけで、フレームは続く。命令の列は送られない
        frame(interp, 0.016, Input::default()).unwrap();
        assert!(rx.try_recv().is_err());
        frame(interp, 0.016, Input::default()).unwrap();
        assert!(rx.try_recv().is_err());
        eval(interp, "@fail = false").unwrap();
        frame(interp, 0.016, Input::default()).unwrap();
        assert!(rx.try_recv().is_ok());
        stop();
    });
}

#[test]
fn options_are_checked() {
    with_canvas(|interp| {
        let ok = eval(
            interp,
            "{\"title\": \"T\", \"width\": 300, \"height\": 200, \"fps\": 30, \"background\": \"#112233\", \"draw\": [] ( 1 ), \"update\": [@dt] ( 1 )}",
        )
        .unwrap();
        let Value::Map(map) = ok else { panic!() };
        let (options, _, update) = parse_options(&map.borrow()).unwrap();
        assert_eq!(options.fps, 30);
        assert_eq!(options.background, [0x11, 0x22, 0x33, 255]);
        assert_eq!(options.window.width, 300.0);
        assert!(update.is_some());

        for (src, expected) in [
            ("{\"update\": [@dt] ( 1 )}", "`draw` がありません"),
            ("{\"draw\": 1}", "関数"),
            ("{\"draw\": [] ( 1 ), \"fps\": 0}", "1〜240"),
            ("{\"draw\": [] ( 1 ), \"fps\": 1.5}", "1〜240"),
            ("{\"draw\": [] ( 1 ), \"width\": 10}", "50 以上"),
            (
                "{\"draw\": [] ( 1 ), \"background\": \"black\"}",
                "`#rrggbb`",
            ),
            ("{\"draw\": [] ( 1 ), \"title\": 1}", "文字列"),
            ("{\"draw\": [] ( 1 ), \"nope\": 1}", "知りません"),
        ] {
            let Value::Map(map) = eval(interp, src).unwrap() else {
                panic!()
            };
            let err = parse_options(&map.borrow()).unwrap_err();
            assert!(err.contains(expected), "{src}: {err}");
        }
    });
}

#[test]
fn colors_and_keys() {
    assert_eq!(parse_color("#000000"), Some([0, 0, 0, 255]));
    assert_eq!(parse_color("#ffffff10"), Some([255, 255, 255, 16]));
    assert_eq!(parse_color("#fff"), None);
    assert_eq!(parse_color("ffffff"), None);
    assert_eq!(parse_color("#ggggggg0"), None);
    for name in ["a", "z", "0", "9", "left", "space", "f12", "shift"] {
        assert!(KEY_NAMES.contains(&name), "{name}");
    }
}
