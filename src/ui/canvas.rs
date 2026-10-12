//! `std:canvas`（design.md 5.25）。フレームごとに `update` と `draw` を呼んで、描いた命令の列を
//! GUI に送る。描画は `canvas_gui.rs`（`ui` feature）

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use super::{Repaint, WindowOptions, host, wake};
use crate::runtime::events::Event;
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;
use crate::stdlib::{expect_args, new_map, num_arg, str_arg, type_error};

/// `[r, g, b, a]`
pub type Color = [u8; 4];

/// 描画の命令（GUI に送る。`Send`）
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Clear(Color),
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
        stroke: Option<f32>,
    },
    Circle {
        x: f32,
        y: f32,
        r: f32,
        color: Color,
        stroke: Option<f32>,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: Color,
        width: f32,
    },
    Polygon {
        points: Vec<(f32, f32)>,
        color: Color,
        stroke: Option<f32>,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        color: Color,
        size: f32,
    },
}

/// 1 フレームの入力の状態
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    pub down: HashSet<String>,
    pub pressed: HashSet<String>,
    pub released: HashSet<String>,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub inside: bool,
    /// 左・右・中ボタンが押されているか
    pub buttons: [bool; 3],
    /// 前のフレームからの間に押されたか
    pub buttons_pressed: [bool; 3],
    pub wheel: f32,
}

/// GUI からインタプリタへのメッセージ
pub enum CanvasMsg {
    /// 次のフレーム。`dt` は前のフレームからの秒数
    Frame {
        dt: f32,
        input: Input,
    },
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanvasOptions {
    pub window: WindowOptions,
    pub background: Color,
    pub fps: u32,
}

/// ホストに渡す、キャンバスのウィンドウを開く依頼
pub struct CanvasRequest {
    pub options: CanvasOptions,
    /// 描いた命令の列。GUI は届いた最新のものを描く
    pub frames: Receiver<Vec<Cmd>>,
    pub events: Sender<Event>,
    pub repaint: Repaint,
    pub quit: Arc<AtomicBool>,
}

struct State {
    update: Option<Value>,
    draw: Value,
    frames: Sender<Vec<Cmd>>,
    repaint: Repaint,
    quit: Arc<AtomicBool>,
    width: f32,
    height: f32,
    input: Input,
    /// `draw` の実行中だけ Some
    cmds: Option<Vec<Cmd>>,
    last_error: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:canvas",
        &[
            ("canvas.run", run),
            ("canvas.quit", quit),
            ("canvas.size", size),
            ("canvas.key_down", |_, a| {
                key("canvas.key_down", a, |i| &i.down)
            }),
            ("canvas.key_pressed", |_, a| {
                key("canvas.key_pressed", a, |i| &i.pressed)
            }),
            ("canvas.key_released", |_, a| {
                key("canvas.key_released", a, |i| &i.released)
            }),
            ("canvas.mouse", mouse),
            ("canvas.clear", clear),
            ("canvas.rect", rect),
            ("canvas.circle", circle),
            ("canvas.line", line),
            ("canvas.polygon", polygon),
            ("canvas.text", text),
        ],
    );
}

// ---- run ----

fn run(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("canvas.run", args, 1)?;
    let Value::Map(options) = &args[0] else {
        return Err(type_error("canvas.run", 0, "マップ", &args[0]));
    };
    let (options, draw, update) = parse_options(&options.borrow())?;
    if !host::claim() {
        return Err(
            "ウィンドウは、1 つのプログラムで 1 回だけ開けます（`ui.run` か `canvas.run`）".into(),
        );
    }
    let Some(host) = host::sender() else {
        return Err("canvas.run は `dolphin` コマンドの中でだけ使えます".into());
    };

    let (frames, frame_rx) = mpsc::channel();
    let repaint = Arc::new(Mutex::new(None));
    let quit = Arc::new(AtomicBool::new(false));
    let request = CanvasRequest {
        options: options.clone(),
        frames: frame_rx,
        events: interp.events.sender(),
        repaint: Arc::clone(&repaint),
        quit: Arc::clone(&quit),
    };
    STATE.with(|state| {
        *state.borrow_mut() = Some(State {
            update,
            draw,
            frames,
            repaint,
            quit,
            width: options.window.width,
            height: options.window.height,
            input: Input::default(),
            cmds: None,
            last_error: None,
        });
    });
    if host.send(host::HostRequest::Canvas(request)).is_err() {
        STATE.with(|state| *state.borrow_mut() = None);
        return Err("ウィンドウを開けません（GUI が動いていません）".into());
    }
    interp
        .events
        .set_ext(Box::new(receive))
        .map_err(|why| format!("canvas.run: {why}"))?;
    let result = interp.run_event_loop();
    interp.events.clear_ext();
    STATE.with(|state| *state.borrow_mut() = None);
    result.map(|()| Value::Nil)
}

fn parse_options(map: &Map) -> Result<(CanvasOptions, Value, Option<Value>), String> {
    let mut window = WindowOptions {
        title: "Dolphin".into(),
        width: 640.0,
        height: 480.0,
        font: None,
    };
    let mut background = [0, 0, 0, 255];
    let mut fps = 60;
    let (mut draw, mut update) = (None, None);
    for (key, value) in map.iter() {
        let wrong = |expected: &str| {
            format!(
                "canvas.run のオプション `{key}` は{expected}です（{}が渡されました）",
                value.type_name()
            )
        };
        match (&**key, value) {
            ("title", Value::Str(s)) => window.title = s.to_string(),
            ("font", Value::Str(s)) => window.font = Some(s.to_string()),
            ("width", Value::Num(n)) if n.is_finite() && *n >= 50.0 => window.width = *n as f32,
            ("height", Value::Num(n)) if n.is_finite() && *n >= 50.0 => window.height = *n as f32,
            ("fps", Value::Num(n)) if n.fract() == 0.0 && (1.0..=240.0).contains(n) => {
                fps = *n as u32;
            }
            ("background", Value::Str(s)) => {
                background = parse_color(s).ok_or_else(|| wrong("`#rrggbb` の文字列"))?;
            }
            ("draw", f @ (Value::Func(_) | Value::Native(_))) => draw = Some(f.clone()),
            ("update", f @ (Value::Func(_) | Value::Native(_))) => update = Some(f.clone()),
            ("width" | "height", _) => return Err(wrong("50 以上の数")),
            ("fps", _) => return Err(wrong("1〜240 の整数")),
            ("title" | "font" | "background", _) => return Err(wrong("文字列")),
            ("draw" | "update", _) => return Err(wrong("関数")),
            _ => return Err(format!("canvas.run のオプション `{key}` は知りません")),
        }
    }
    let draw = draw.ok_or("canvas.run に `draw` がありません")?;
    Ok((
        CanvasOptions {
            window,
            background,
            fps,
        },
        draw,
        update,
    ))
}

/// GUI からのメッセージ（イベントループの `Ext`）
fn receive(interp: &mut Interpreter<'_>, payload: Box<dyn Any + Send>) -> Result<(), String> {
    let Ok(msg) = payload.downcast::<CanvasMsg>() else {
        return Ok(());
    };
    match *msg {
        CanvasMsg::Closed => interp.events.stop(),
        CanvasMsg::Frame { dt, input } => frame(interp, dt, input)?,
    }
    Ok(())
}

/// 1 フレーム: `update[dt]`、`draw[]`、命令の列を GUI へ
fn frame(interp: &mut Interpreter<'_>, dt: f32, input: Input) -> Result<(), String> {
    let Some((update, draw)) = STATE.with(|state| {
        state.borrow_mut().as_mut().map(|s| {
            s.input = input;
            (s.update.clone(), s.draw.clone())
        })
    }) else {
        return Ok(());
    };
    if let Some(update) = update {
        call(interp, &update, vec![Value::Num(f64::from(dt))])?;
    }
    STATE.with(|state| {
        if let Some(s) = state.borrow_mut().as_mut() {
            s.cmds = Some(Vec::new());
        }
    });
    let drawn = call(interp, &draw, Vec::new());
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(s) = state.as_mut() else {
            return;
        };
        let cmds = s.cmds.take();
        // 失敗したフレームは送らない（前のフレームのまま）
        if matches!(drawn, Ok(true))
            && let Some(cmds) = cmds
        {
            let _ = s.frames.send(cmds);
            wake(&s.repaint);
        }
    });
    // 次のフレームを要求できるよう、失敗しても GUI を起こす（待ちを解く）
    STATE.with(|state| {
        if let Some(s) = state.borrow().as_ref() {
            wake(&s.repaint);
        }
    });
    drawn.map(|_| ())
}

/// 関数を呼ぶ。エラーは標準エラーに出す（同じメッセージが続くときは最初の 1 回だけ）。
/// 成功なら Ok(true)、エラーを報告して続けるなら Ok(false)、`os.exit` なら Err
fn call(interp: &mut Interpreter<'_>, f: &Value, args: Vec<Value>) -> Result<bool, String> {
    match interp.call_value(f, args) {
        Ok(_) => {
            STATE.with(|state| {
                if let Some(s) = state.borrow_mut().as_mut() {
                    s.last_error = None;
                }
            });
            Ok(true)
        }
        Err(message) => {
            if interp.exit_code.is_some() {
                return Err(message);
            }
            let text = match interp.take_native_error() {
                Some(e) => super::describe_error(e),
                None => format!("canvas: コールバックでエラー: {message}"),
            };
            let repeated = STATE.with(|state| {
                state
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|s| s.last_error.replace(text.clone()).as_deref() == Some(&text))
            });
            if !repeated {
                eprintln!("{text}");
            }
            Ok(false)
        }
    }
}

// ---- 入力と画面の情報 ----

/// 知っているキーの名前（design.md 5.25）
pub(super) const KEY_NAMES: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "left",
    "right",
    "up",
    "down",
    "space",
    "enter",
    "escape",
    "tab",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "shift",
    "ctrl",
    "alt",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

/// 画面が開いている間のいまの入力から、`pick` で選んだ集合にキーがあるか
fn key(name: &str, args: &[Value], pick: fn(&Input) -> &HashSet<String>) -> Result<Value, String> {
    expect_args(name, args, 1)?;
    let key = str_arg(name, args, 0)?;
    if !KEY_NAMES.contains(&key) {
        return Err(format!("{name} の知らないキーの名前 `{key}` です"));
    }
    with_state(name, |s| Value::Bool(pick(&s.input).contains(key)))
}

fn with_state<T>(name: &str, f: impl FnOnce(&mut State) -> T) -> Result<T, String> {
    STATE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .map(f)
            .ok_or_else(|| format!("{name} は `canvas.run` の中でだけ使えます"))
    })
}

fn mouse(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("canvas.mouse", args, 0)?;
    with_state("canvas.mouse", |s| {
        let i = &s.input;
        let mut map = Map::default();
        map.insert("x".into(), Value::Num(f64::from(i.mouse_x)));
        map.insert("y".into(), Value::Num(f64::from(i.mouse_y)));
        map.insert("inside".into(), Value::Bool(i.inside));
        for (n, name) in ["left", "right", "middle"].iter().enumerate() {
            map.insert((*name).into(), Value::Bool(i.buttons[n]));
        }
        for (n, name) in ["left_pressed", "right_pressed", "middle_pressed"]
            .iter()
            .enumerate()
        {
            map.insert((*name).into(), Value::Bool(i.buttons_pressed[n]));
        }
        map.insert("wheel".into(), Value::Num(f64::from(i.wheel)));
        new_map(map)
    })
}

fn size(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("canvas.size", args, 0)?;
    with_state("canvas.size", |s| {
        let mut map = Map::default();
        map.insert("width".into(), Value::Num(f64::from(s.width)));
        map.insert("height".into(), Value::Num(f64::from(s.height)));
        new_map(map)
    })
}

fn quit(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("canvas.quit", args, 0)?;
    with_state("canvas.quit", |s| {
        s.quit.store(true, Ordering::SeqCst);
        wake(&s.repaint);
        Value::Nil
    })
}

// ---- 描画 ----

/// `#rrggbb` か `#rrggbbaa`
pub fn parse_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([
        byte(0)?,
        byte(2)?,
        byte(4)?,
        if hex.len() == 8 { byte(6)? } else { 255 },
    ])
}

fn color_arg(name: &str, args: &[Value], i: usize) -> Result<Color, String> {
    let s = str_arg(name, args, i)?;
    parse_color(s).ok_or_else(|| {
        format!(
            "{name} の {} 番目の引数は `#rrggbb` か `#rrggbbaa` の文字列です（{s:?}）",
            i + 1
        )
    })
}

fn float_arg(name: &str, args: &[Value], i: usize) -> Result<f32, String> {
    let n = num_arg(name, args, i)?;
    if n.is_finite() {
        Ok(n as f32)
    } else {
        Err(format!(
            "{name} の {} 番目の引数は有限の数です（{n}）",
            i + 1
        ))
    }
}

/// `options`（`{"stroke": 太さ}`）。省略なら塗りつぶし
fn stroke_option(name: &str, args: &[Value], i: usize) -> Result<Option<f32>, String> {
    let Some(value) = args.get(i) else {
        return Ok(None);
    };
    let Value::Map(map) = value else {
        return Err(type_error(name, i, "マップ", value));
    };
    let mut stroke = None;
    for (key, value) in map.borrow().iter() {
        match (&**key, value) {
            ("stroke", Value::Num(n)) if n.is_finite() && *n > 0.0 => stroke = Some(*n as f32),
            ("stroke", _) => {
                return Err(format!(
                    "{name} のオプション `stroke` は 0 より大きい数です"
                ));
            }
            _ => return Err(format!("{name} のオプション `{key}` は知りません")),
        }
    }
    Ok(stroke)
}

fn args_between(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(format!(
            "{name} の引数は {min} 個から {max} 個です（{} 個渡されました）",
            args.len()
        ))
    }
}

fn push(name: &str, cmd: Cmd) -> Result<Value, String> {
    STATE.with(
        |state| match state.borrow_mut().as_mut().and_then(|s| s.cmds.as_mut()) {
            Some(cmds) => {
                cmds.push(cmd);
                Ok(Value::Nil)
            }
            None => Err(format!("{name} は `draw` の中でだけ使えます")),
        },
    )
}

fn clear(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("canvas.clear", args, 1)?;
    let color = color_arg("canvas.clear", args, 0)?;
    push("canvas.clear", Cmd::Clear(color))
}

fn rect(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("canvas.rect", args, 5, 6)?;
    let n = |i| float_arg("canvas.rect", args, i);
    let cmd = Cmd::Rect {
        x: n(0)?,
        y: n(1)?,
        w: n(2)?,
        h: n(3)?,
        color: color_arg("canvas.rect", args, 4)?,
        stroke: stroke_option("canvas.rect", args, 5)?,
    };
    push("canvas.rect", cmd)
}

fn circle(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("canvas.circle", args, 4, 5)?;
    let n = |i| float_arg("canvas.circle", args, i);
    let cmd = Cmd::Circle {
        x: n(0)?,
        y: n(1)?,
        r: n(2)?,
        color: color_arg("canvas.circle", args, 3)?,
        stroke: stroke_option("canvas.circle", args, 4)?,
    };
    push("canvas.circle", cmd)
}

fn line(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("canvas.line", args, 5, 6)?;
    let n = |i| float_arg("canvas.line", args, i);
    let width = if args.len() == 6 { n(5)? } else { 1.0 };
    let cmd = Cmd::Line {
        x1: n(0)?,
        y1: n(1)?,
        x2: n(2)?,
        y2: n(3)?,
        color: color_arg("canvas.line", args, 4)?,
        width,
    };
    push("canvas.line", cmd)
}

fn polygon(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("canvas.polygon", args, 2, 3)?;
    let Value::Array(list) = &args[0] else {
        return Err(type_error("canvas.polygon", 0, "配列", &args[0]));
    };
    let mut points = Vec::new();
    for (i, item) in list.borrow().iter().enumerate() {
        let pair = match item {
            Value::Array(p) => p.borrow().clone(),
            _ => Vec::new(),
        };
        match pair.as_slice() {
            [Value::Num(x), Value::Num(y)] if x.is_finite() && y.is_finite() => {
                points.push((*x as f32, *y as f32));
            }
            _ => {
                return Err(format!(
                    "canvas.polygon の点 {i} 番目は `{{x, y}}`（数 2 つの配列）です"
                ));
            }
        }
    }
    if points.len() < 3 {
        return Err(format!(
            "canvas.polygon の点は 3 つ以上です（{} 個）",
            points.len()
        ));
    }
    let cmd = Cmd::Polygon {
        points,
        color: color_arg("canvas.polygon", args, 1)?,
        stroke: stroke_option("canvas.polygon", args, 2)?,
    };
    push("canvas.polygon", cmd)
}

fn text(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    args_between("canvas.text", args, 4, 5)?;
    let n = |i| float_arg("canvas.text", args, i);
    let size = if args.len() == 5 { n(4)? } else { 14.0 };
    if size <= 0.0 {
        return Err(format!(
            "canvas.text の大きさは 0 より大きい数です（{size}）"
        ));
    }
    let cmd = Cmd::Text {
        x: n(0)?,
        y: n(1)?,
        text: args[2].to_string(),
        color: color_arg("canvas.text", args, 3)?,
        size,
    };
    push("canvas.text", cmd)
}

#[cfg(test)]
mod tests;
