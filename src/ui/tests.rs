//! 部品の木への変換と、操作のコールバックの往復のテスト（描画は手で確かめる）

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use super::*;
use crate::syntax::{lexer, parser};

/// `src` を実行して、最後の式の値を返す（`std:ui` を登録した実行環境で）
fn eval(interp: &mut Interpreter<'_>, src: &str) -> Result<Value, String> {
    let tokens = lexer::tokenize(src).map_err(|e| e.message)?;
    let program = parser::parse(&tokens).map_err(|e| e.message)?;
    interp
        .run(&program, true)
        .map(|v| v.unwrap_or(Value::Nil))
        .map_err(|e| e.message)
}

fn with_ui(test: impl FnOnce(&mut Interpreter<'_>)) {
    let mut out = Vec::new();
    let mut interp = Interpreter::new(&mut out, ".".into());
    crate::stdlib::register(&mut interp);
    super::register(&mut interp);
    test(&mut interp);
}

fn node(interp: &mut Interpreter<'_>, src: &str) -> (Node, HashMap<u64, Value>) {
    let value = eval(interp, src).unwrap();
    let mut callbacks = HashMap::new();
    let mut next_id = 1;
    let node = to_node(&value, &mut callbacks, &mut next_id).unwrap();
    (node, callbacks)
}

#[test]
fn builds_a_tree_with_numbered_callbacks() {
    with_ui(|interp| {
        let (tree, callbacks) = node(
            interp,
            r##"@ui := import["std:ui"]
@ui.column[{
    @ui.text["こんにちは", {"size": 20, "color": "#ff8000", "bold": true}],
    nil,
    @ui.row[{
        @ui.button["+", [] ( 1 )],
        @ui.button["-", [] ( 2 ), {"enabled": false}]
    }, {"spacing": 4}],
    @ui.text_input["abc", [@v] ( @v ), {"hint": "名前", "password": true}],
    @ui.checkbox["同意", true, [@b] ( @b )],
    @ui.slider[0.5, 0, 1, [@x] ( @x )],
    @ui.select[{"a", "b"}, 1, [@i] ( @i )],
    @ui.progress[2],
    @ui.group[{@ui.separator[], @ui.space[8]}],
    @ui.scroll[@ui.text[42]]
}]"##,
        );
        let Node::Column { children, spacing } = tree else {
            panic!("column ではありません");
        };
        assert_eq!(spacing, None);
        // nil は置かない
        assert_eq!(children.len(), 9);
        assert_eq!(
            children[0],
            Node::Text {
                text: "こんにちは".into(),
                size: Some(20.0),
                color: Some([0xff, 0x80, 0x00]),
                bold: true
            }
        );
        let Node::Row {
            children: buttons,
            spacing,
        } = &children[1]
        else {
            panic!("row ではありません");
        };
        assert_eq!(*spacing, Some(4.0));
        assert_eq!(
            buttons[0],
            Node::Button {
                label: "+".into(),
                id: 1,
                enabled: true
            }
        );
        assert_eq!(
            buttons[1],
            Node::Button {
                label: "-".into(),
                id: 2,
                enabled: false
            }
        );
        assert_eq!(
            children[2],
            Node::TextInput {
                value: "abc".into(),
                id: 3,
                hint: "名前".into(),
                multiline: false,
                password: true
            }
        );
        assert_eq!(
            children[3],
            Node::Checkbox {
                label: "同意".into(),
                checked: true,
                id: 4
            }
        );
        assert_eq!(
            children[4],
            Node::Slider {
                value: 0.5,
                min: 0.0,
                max: 1.0,
                id: 5
            }
        );
        assert_eq!(
            children[5],
            Node::Select {
                labels: vec!["a".into(), "b".into()],
                index: 1,
                id: 6
            }
        );
        // 範囲外は 0〜1 に収める
        assert_eq!(children[6], Node::Progress(1.0));
        assert_eq!(
            children[7],
            Node::Group(vec![Node::Separator, Node::Space(8.0)])
        );
        // 文字列以外の `text` は、表示用の文字列にする
        assert_eq!(
            children[8],
            Node::Scroll(Box::new(Node::Text {
                text: "42".into(),
                size: None,
                color: None,
                bold: false
            }))
        );
        assert_eq!(callbacks.len(), 6);
    });
}

#[test]
fn arrays_and_nil_are_columns() {
    with_ui(|interp| {
        let (tree, _) = node(interp, "@ui := import[\"std:ui\"]\n{@ui.separator[], nil}");
        assert_eq!(
            tree,
            Node::Column {
                children: vec![Node::Separator],
                spacing: None
            }
        );
        let (tree, _) = node(interp, "nil");
        assert_eq!(
            tree,
            Node::Column {
                children: vec![],
                spacing: None
            }
        );
    });
}

#[test]
fn argument_errors() {
    with_ui(|interp| {
        let cases = [
            ("@ui.button[\"x\", 5]", "関数"),
            (
                "@ui.button[\"x\", [] ( 1 ), {\"nope\": 1}]",
                "知らないオプション",
            ),
            ("@ui.text[\"x\", {\"color\": \"red\"}]", "`#rrggbb`"),
            ("@ui.text[\"x\", {\"size\": \"big\"}]", "数値"),
            ("@ui.slider[0, 1, 1, [@x] ( 1 )]", "min < max"),
            ("@ui.select[{\"a\"}, 3, [@x] ( 1 )]", "番号"),
            ("@ui.column[5]", "配列"),
            ("@ui.checkbox[\"x\", 1, [@x] ( 1 )]", "真偽値"),
            ("@ui.space[-1]", "0 以上"),
        ];
        eval(interp, "@ui := import[\"std:ui\"]").unwrap();
        for (src, expected) in cases {
            let err = eval(interp, src).unwrap_err();
            assert!(err.contains(expected), "{src}: {err}");
        }
        // 部品でない値は、view を呼んだ時点のエラー
        let value = eval(interp, "{1}").unwrap();
        let err = to_node(&value, &mut HashMap::new(), &mut 1).unwrap_err();
        assert!(err.contains("部品"), "{err}");
        let value = eval(interp, "{\"type\": \"nope\"}").unwrap();
        assert!(to_node(&value, &mut HashMap::new(), &mut 1).is_err());
    });
}

#[test]
fn clicks_run_callbacks_and_rerender() {
    with_ui(|interp| {
        eval(
            interp,
            r#"@ui := import["std:ui"]
@count := 0
@name := ""
@view := [] (
    # @ui.column[{
        @ui.text["回数: @count / @name"],
        @ui.button["+", [] ( @count = @count + 1 )],
        @ui.text_input[@name, [@v] ( @name = @v )]
    }]
)"#,
        )
        .unwrap();
        let view = eval(interp, "@view").unwrap();

        // `ui.run` の準備だけを、GUI なしで再現する
        let (trees, tree_rx) = mpsc::channel();
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                view,
                trees,
                repaint: Arc::new(Mutex::new(None)),
                quit: Arc::new(AtomicBool::new(false)),
                callbacks: HashMap::new(),
            });
        });
        render(interp);
        let first = tree_rx.try_recv().unwrap();
        let ids = button_and_input_ids(&first, "回数: 0 / ");

        // ボタンを押す → 状態が変わり、新しい画面が届く
        receive(
            interp,
            Box::new(UiMsg::Callback {
                id: ids.0,
                arg: None,
            }),
        )
        .unwrap();
        let second = tree_rx.try_recv().unwrap();
        button_and_input_ids(&second, "回数: 1 / ");

        // 入力 → 引数つきで呼ばれる
        receive(
            interp,
            Box::new(UiMsg::Callback {
                id: ids.1,
                arg: Some(Data::Str("イルカ".into())),
            }),
        )
        .unwrap();
        let third = tree_rx.try_recv().unwrap();
        button_and_input_ids(&third, "回数: 1 / イルカ");

        // 番号は、構造が同じなら変わらない（画面が届く前の操作も、同じ部品に届く）
        assert_eq!(
            button_and_input_ids(&second, "回数: 1 / "),
            button_and_input_ids(&third, "回数: 1 / イルカ")
        );

        // 知らない番号の操作は、何もしない。エラーになったコールバックでも続く
        receive(interp, Box::new(UiMsg::Callback { id: 999, arg: None })).unwrap();
        assert!(tree_rx.try_recv().is_err());

        // ウィンドウが閉じられたら、イベントループを止める
        receive(interp, Box::new(UiMsg::Closed)).unwrap();
        STATE.with(|state| *state.borrow_mut() = None);
    });
}

/// 画面から (ボタンの番号, 入力欄の番号) を取り出しながら、最初の文字が期待どおりか確かめる
fn button_and_input_ids(tree: &Node, text: &str) -> (u64, u64) {
    let Node::Column { children, .. } = tree else {
        panic!("{tree:?}");
    };
    let Node::Text { text: shown, .. } = &children[0] else {
        panic!("{children:?}");
    };
    assert_eq!(shown, text);
    let (Node::Button { id: button, .. }, Node::TextInput { id: input, .. }) =
        (&children[1], &children[2])
    else {
        panic!("{children:?}");
    };
    (*button, *input)
}

#[test]
fn view_errors_keep_the_previous_screen() {
    with_ui(|interp| {
        eval(
            interp,
            r#"@ui := import["std:ui"]
@fail := false
@view := [] (
    if @fail ( @ui.nope[] )
    # @ui.text["ok"]
)"#,
        )
        .unwrap();
        let view = eval(interp, "@view").unwrap();
        let (trees, tree_rx) = mpsc::channel();
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                view,
                trees,
                repaint: Arc::new(Mutex::new(None)),
                quit: Arc::new(AtomicBool::new(false)),
                callbacks: HashMap::new(),
            });
        });
        render(interp);
        assert!(tree_rx.try_recv().is_ok());
        eval(interp, "@fail = true").unwrap();
        render(interp);
        // 新しい画面は届かない（前の画面のまま）
        assert!(tree_rx.try_recv().is_err());
        STATE.with(|state| *state.borrow_mut() = None);
    });
}

#[test]
fn window_options_are_checked() {
    with_ui(|interp| {
        let value = eval(
            interp,
            "{\"title\": \"T\", \"width\": 300, \"height\": 200}",
        )
        .unwrap();
        let options = window_options(&value).unwrap();
        assert_eq!(
            options,
            WindowOptions {
                title: "T".into(),
                width: 300.0,
                height: 200.0,
                font: None
            }
        );
        for (src, expected) in [
            ("{\"width\": 10}", "50 以上"),
            ("{\"title\": 1}", "文字列"),
            ("{\"nope\": 1}", "知りません"),
        ] {
            let value = eval(interp, src).unwrap();
            let err = window_options(&value).unwrap_err();
            assert!(err.contains(expected), "{err}");
        }
    });
}

#[test]
fn colors() {
    assert_eq!(parse_color("#000000"), Some([0, 0, 0]));
    assert_eq!(parse_color("#FfA010"), Some([255, 160, 16]));
    assert_eq!(parse_color("#fff"), None);
    assert_eq!(parse_color("ff0000"), None);
    assert_eq!(parse_color("#gg0000"), None);
}
