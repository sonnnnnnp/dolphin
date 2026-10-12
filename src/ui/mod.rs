//! `std:ui`（design.md 5.24）
//!
//! 画面は、`view` 関数が返す部品の値（マップ）を `Node` の木にして GUI に送る。GUI（egui）は
//! メインスレッドで動き、操作はイベントループ（5.23）にメッセージで届く。関数値は GUI に
//! 持ち出さず、番号で呼ぶ。この `mod.rs` は描画に依存せず、`gui.rs`（`ui` feature）だけが egui を使う

pub mod canvas;
pub mod host;
mod widgets;

#[cfg(feature = "ui")]
mod canvas_gui;
#[cfg(feature = "ui")]
mod gui;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::diag::Diagnostic;
use crate::runtime::data::Data;
use crate::runtime::error::RuntimeError;
use crate::runtime::events::Event;
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;
use crate::stdlib::{expect_args, func_arg, type_error};

/// `std:ui` と `std:canvas` を登録する
pub fn register(interp: &mut Interpreter<'_>) {
    widgets::register(interp);
    canvas::register(interp);
}

/// 画面の部品（GUI に送る。`Send`）
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Text {
        text: String,
        size: Option<f32>,
        color: Option<[u8; 3]>,
        bold: bool,
    },
    Button {
        label: String,
        id: u64,
        enabled: bool,
    },
    TextInput {
        value: String,
        id: u64,
        hint: String,
        multiline: bool,
        password: bool,
    },
    Checkbox {
        label: String,
        checked: bool,
        id: u64,
    },
    Slider {
        value: f64,
        min: f64,
        max: f64,
        id: u64,
    },
    Select {
        labels: Vec<String>,
        index: usize,
        id: u64,
    },
    Progress(f32),
    Column {
        children: Vec<Node>,
        spacing: Option<f32>,
    },
    Row {
        children: Vec<Node>,
        spacing: Option<f32>,
    },
    Group(Vec<Node>),
    Scroll(Box<Node>),
    Separator,
    Space(f32),
}

/// 画面が届いたときに GUI を起こす関数の置き場（GUI が準備できたら入る）
pub type Repaint = Arc<Mutex<Option<Box<dyn Fn() + Send>>>>;

/// GUI からインタプリタへのメッセージ
pub enum UiMsg {
    /// 部品の操作。`id` のコールバックを、`arg`（あれば）を渡して呼ぶ
    Callback { id: u64, arg: Option<Data> },
    /// ウィンドウが閉じられた
    Closed,
}

/// `ui.run` の `options`
#[derive(Debug, Clone, PartialEq)]
pub struct WindowOptions {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub font: Option<String>,
}

/// ホスト（メインスレッド）に渡す、ウィンドウを開く依頼
pub struct WindowRequest {
    pub options: WindowOptions,
    /// 新しい画面。GUI は届いた最新のものを描く
    pub trees: Receiver<Node>,
    /// 操作を届ける先（インタプリタのイベントループ）
    pub events: Sender<Event>,
    /// 画面が届いたときに GUI を起こす（GUI が準備できたら入る）
    pub repaint: Repaint,
    /// true になったら、GUI はウィンドウを閉じる
    pub quit: Arc<AtomicBool>,
}

/// `ui.run` の間だけ、インタプリタのスレッドが持つ状態
struct State {
    view: Value,
    trees: Sender<Node>,
    repaint: Repaint,
    quit: Arc<AtomicBool>,
    /// 最新の画面のコールバック（番号 → 関数値）
    callbacks: HashMap<u64, Value>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn wake(repaint: &Repaint) {
    if let Ok(guard) = repaint.lock()
        && let Some(f) = guard.as_ref()
    {
        f();
    }
}

// ---- ui.run ----

pub(crate) fn run(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "ui.run の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let view = func_arg("ui.run", args, 0)?.clone();
    let options = match args.get(1) {
        Some(value) => window_options(value)?,
        None => WindowOptions {
            title: "Dolphin".into(),
            width: 640.0,
            height: 480.0,
            font: None,
        },
    };
    if !host::claim() {
        return Err(
            "ウィンドウは、1 つのプログラムで 1 回だけ開けます（`ui.run` か `canvas.run`）".into(),
        );
    }
    let Some(host) = host::sender() else {
        return Err("ui.run は `dolphin` コマンドの中でだけ使えます".into());
    };

    let (trees, tree_rx) = mpsc::channel();
    let repaint = Arc::new(Mutex::new(None));
    let quit = Arc::new(AtomicBool::new(false));
    let request = WindowRequest {
        options,
        trees: tree_rx,
        events: interp.events.sender(),
        repaint: Arc::clone(&repaint),
        quit: Arc::clone(&quit),
    };
    STATE.with(|state| {
        *state.borrow_mut() = Some(State {
            view,
            trees,
            repaint,
            quit,
            callbacks: HashMap::new(),
        });
    });
    // 最初の画面を作ってから、ウィンドウを開く依頼を出す
    render(interp);
    if host.send(host::HostRequest::Ui(request)).is_err() {
        STATE.with(|state| *state.borrow_mut() = None);
        return Err("ウィンドウを開けません（GUI が動いていません）".into());
    }

    interp
        .events
        .set_ext(Box::new(receive))
        .map_err(|why| format!("ui.run: {why}"))?;
    let result = interp.run_event_loop();
    interp.events.clear_ext();
    STATE.with(|state| *state.borrow_mut() = None);
    result.map(|()| Value::Nil)
}

fn window_options(value: &Value) -> Result<WindowOptions, String> {
    let Value::Map(map) = value else {
        return Err(type_error("ui.run", 1, "マップ", value));
    };
    let mut options = WindowOptions {
        title: "Dolphin".into(),
        width: 640.0,
        height: 480.0,
        font: None,
    };
    for (key, value) in map.borrow().iter() {
        match (&**key, value) {
            ("title", Value::Str(s)) => options.title = s.to_string(),
            ("font", Value::Str(s)) => options.font = Some(s.to_string()),
            ("width", Value::Num(n)) if *n >= 50.0 && n.is_finite() => options.width = *n as f32,
            ("height", Value::Num(n)) if *n >= 50.0 && n.is_finite() => options.height = *n as f32,
            ("width" | "height", other) => {
                return Err(format!(
                    "ui.run のオプション `{key}` は 50 以上の数です（{}が渡されました）",
                    describe(other)
                ));
            }
            ("title" | "font", other) => {
                return Err(format!(
                    "ui.run のオプション `{key}` は文字列です（{}が渡されました）",
                    other.type_name()
                ));
            }
            _ => return Err(format!("ui.run のオプション `{key}` は知りません")),
        }
    }
    Ok(options)
}

fn describe(value: &Value) -> String {
    match value {
        Value::Num(n) => n.to_string(),
        other => other.type_name().to_string(),
    }
}

/// GUI からのメッセージ（イベントループの `Ext`）
fn receive(interp: &mut Interpreter<'_>, payload: Box<dyn Any + Send>) -> Result<(), String> {
    let Ok(msg) = payload.downcast::<UiMsg>() else {
        return Ok(());
    };
    match *msg {
        UiMsg::Closed => interp.events.stop(),
        UiMsg::Callback { id, arg } => {
            let callback = STATE.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .and_then(|s| s.callbacks.get(&id).cloned())
            });
            // 1 つ前の画面のコールバックは、もうない
            let Some(callback) = callback else {
                return Ok(());
            };
            let args = arg.map(|a| vec![a.into_value()]).unwrap_or_default();
            if let Err(message) = interp.call_value(&callback, args) {
                if interp.exit_code.is_some() {
                    return Err(message);
                }
                report(interp, &message);
            }
            render(interp);
        }
    }
    Ok(())
}

/// 標準エラーに、位置と呼び出し履歴つきでエラーを出す
fn report(interp: &mut Interpreter<'_>, message: &str) {
    match interp.take_native_error() {
        Some(e) => eprintln!("{}", describe_error(e)),
        None => eprintln!("ui: コールバックでエラー: {message}"),
    }
}

pub(super) fn describe_error(e: RuntimeError) -> String {
    let diag = Diagnostic::from(e);
    let mut text = format!(
        "ui: エラー: {}:{}: {}",
        diag.span.line, diag.span.col, diag.message
    );
    for note in &diag.notes {
        text.push_str(&format!("\n  {note}"));
    }
    text
}

// ---- 画面の組み立て ----

/// `view` を呼んで、新しい画面を GUI に送る。失敗したら、標準エラーに出して、画面は前のまま
pub(crate) fn render(interp: &mut Interpreter<'_>) {
    let Some(view) = STATE.with(|state| state.borrow().as_ref().map(|s| s.view.clone())) else {
        return;
    };
    let value = match interp.call_value(&view, Vec::new()) {
        Ok(value) => value,
        Err(message) => {
            report(interp, &message);
            return;
        }
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return;
        };
        // 部品の番号は、画面の中の順番で決める。構造が同じ画面なら、同じ部品に同じ番号がつく
        // （GUI に新しい画面が届く前の操作も、同じ部品のコールバックに届く）
        let mut callbacks = HashMap::new();
        match to_node(&value, &mut callbacks, &mut 1) {
            Ok(node) => {
                state.callbacks = callbacks;
                let _ = state.trees.send(node);
                wake(&state.repaint);
            }
            Err(message) => eprintln!("ui: view の戻り値が正しくありません: {message}"),
        }
    });
}

/// `ui.refresh[]`
pub(crate) fn refresh(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.refresh", args, 0)?;
    render(interp);
    Ok(Value::Nil)
}

/// `ui.quit[]`
pub(crate) fn quit(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ui.quit", args, 0)?;
    STATE.with(|state| {
        if let Some(state) = state.borrow().as_ref() {
            state.quit.store(true, Ordering::SeqCst);
            wake(&state.repaint);
        }
    });
    Ok(Value::Nil)
}

/// 部品の値（マップ）を `Node` にする。コールバックは番号をつけて `callbacks` に入れる
pub(crate) fn to_node(
    value: &Value,
    callbacks: &mut HashMap<u64, Value>,
    next_id: &mut u64,
) -> Result<Node, String> {
    let mut register = |callback: &Value| {
        let id = *next_id;
        *next_id += 1;
        callbacks.insert(id, callback.clone());
        id
    };
    match value {
        Value::Nil => Ok(Node::Column {
            children: Vec::new(),
            spacing: None,
        }),
        Value::Array(items) => {
            let children = nodes(&items.borrow(), callbacks, next_id)?;
            Ok(Node::Column {
                children,
                spacing: None,
            })
        }
        Value::Map(map) => {
            let map = map.borrow();
            let kind = match map.get("type") {
                Some(Value::Str(kind)) => kind.to_string(),
                _ => return Err("部品ではないマップです（`std:ui` の関数で作ります）".into()),
            };
            let str_of = |key: &str| match map.get(key) {
                Some(Value::Str(s)) => s.to_string(),
                _ => String::new(),
            };
            let num_of = |key: &str| match map.get(key) {
                Some(Value::Num(n)) => *n,
                _ => 0.0,
            };
            let bool_of = |key: &str| matches!(map.get(key), Some(Value::Bool(true)));
            let callback = |key: &str| map.get(key).cloned().unwrap_or(Value::Nil);
            let children = |key: &str| match map.get(key) {
                Some(Value::Array(items)) => Some(items.borrow().clone()),
                _ => None,
            };
            let spacing = || match map.get("spacing") {
                Some(Value::Num(n)) => Some(*n as f32),
                _ => None,
            };
            Ok(match kind.as_str() {
                "text" => Node::Text {
                    text: str_of("text"),
                    size: match map.get("size") {
                        Some(Value::Num(n)) => Some(*n as f32),
                        _ => None,
                    },
                    color: match map.get("color") {
                        Some(Value::Str(s)) => parse_color(s),
                        _ => None,
                    },
                    bold: bool_of("bold"),
                },
                "button" => Node::Button {
                    label: str_of("label"),
                    id: register(&callback("on_click")),
                    enabled: !matches!(map.get("enabled"), Some(Value::Bool(false))),
                },
                "text_input" => Node::TextInput {
                    value: str_of("value"),
                    id: register(&callback("on_change")),
                    hint: str_of("hint"),
                    multiline: bool_of("multiline"),
                    password: bool_of("password"),
                },
                "checkbox" => Node::Checkbox {
                    label: str_of("label"),
                    checked: bool_of("checked"),
                    id: register(&callback("on_change")),
                },
                "slider" => Node::Slider {
                    value: num_of("value"),
                    min: num_of("min"),
                    max: num_of("max"),
                    id: register(&callback("on_change")),
                },
                "select" => {
                    let labels = children("labels")
                        .unwrap_or_default()
                        .iter()
                        .map(|v| v.to_string())
                        .collect();
                    Node::Select {
                        labels,
                        index: num_of("index") as usize,
                        id: register(&callback("on_change")),
                    }
                }
                "progress" => Node::Progress(num_of("fraction") as f32),
                "column" | "row" | "group" => {
                    let items = children("children").unwrap_or_default();
                    let nodes = nodes(&items, callbacks, next_id)?;
                    match kind.as_str() {
                        "column" => Node::Column {
                            children: nodes,
                            spacing: spacing(),
                        },
                        "row" => Node::Row {
                            children: nodes,
                            spacing: spacing(),
                        },
                        _ => Node::Group(nodes),
                    }
                }
                "scroll" => {
                    let child = map.get("child").cloned().unwrap_or(Value::Nil);
                    Node::Scroll(Box::new(to_node(&child, callbacks, next_id)?))
                }
                "separator" => Node::Separator,
                "space" => Node::Space(num_of("size") as f32),
                other => return Err(format!("知らない部品 `{other}` です")),
            })
        }
        other => Err(format!(
            "部品は `std:ui` の関数が返す値です（{}でした）",
            other.type_name()
        )),
    }
}

/// 子の部品の並び。`nil` は、その場所に何も置かない
fn nodes(
    items: &[Value],
    callbacks: &mut HashMap<u64, Value>,
    next_id: &mut u64,
) -> Result<Vec<Node>, String> {
    items
        .iter()
        .filter(|item| !matches!(item, Value::Nil))
        .map(|item| to_node(item, callbacks, next_id))
        .collect()
}

/// `#rrggbb`
fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests;
