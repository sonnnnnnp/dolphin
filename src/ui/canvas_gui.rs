//! `std:canvas` の描画（egui）。メインスレッドで動く（design.md 5.25）

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;

use super::canvas::{CanvasMsg, CanvasRequest, Cmd, Color, Input, KEY_NAMES};
use crate::runtime::events::Event;

/// ウィンドウを開いて、閉じられるまで戻らない。閉じたら、インタプリタに知らせる
pub fn run_window(request: CanvasRequest) {
    let CanvasRequest {
        options,
        frames,
        events,
        repaint,
        quit,
    } = request;
    let window = &options.window;
    let viewport = egui::ViewportBuilder::default()
        .with_title(window.title.clone())
        .with_inner_size([window.width, window.height]);
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let app_events = events.clone();
    let font = window.font.clone();
    let title = window.title.clone();
    let (width, height) = (window.width, window.height);
    let (background, fps) = (options.background, options.fps);
    let result = eframe::run_native(
        &title,
        native,
        Box::new(move |cc| {
            super::gui::setup_fonts(&cc.egui_ctx, font.as_deref());
            let ctx = cc.egui_ctx.clone();
            if let Ok(mut slot) = repaint.lock() {
                *slot = Some(Box::new(move || ctx.request_repaint()));
            }
            Ok(Box::new(App {
                frames,
                events: app_events,
                quit,
                size: (width, height),
                background,
                interval: Duration::from_secs_f64(1.0 / f64::from(fps)),
                cmds: Vec::new(),
                waiting: false,
                last_sent: None,
                pressed: HashSet::new(),
                released: HashSet::new(),
                buttons_pressed: [false; 3],
                wheel: 0.0,
                images: Images::default(),
            }))
        }),
    );
    if let Err(e) = result {
        eprintln!("canvas: ウィンドウを開けません: {e}");
    }
    let _ = events.send(Event::Ext(Box::new(CanvasMsg::Closed)));
}

struct App {
    frames: Receiver<Vec<Cmd>>,
    events: Sender<Event>,
    quit: Arc<AtomicBool>,
    /// 論理的な画面の大きさ
    size: (f32, f32),
    background: Color,
    interval: Duration,
    cmds: Vec<Cmd>,
    /// フレームを要求して、まだ命令の列が届いていない
    waiting: bool,
    last_sent: Option<Instant>,
    /// 前のフレームを要求してからの間に起きたこと
    pressed: HashSet<String>,
    released: HashSet<String>,
    buttons_pressed: [bool; 3],
    wheel: f32,
    images: Images,
}

/// 論理座標と、画面（ウィンドウの中の描く領域）の対応
struct Layout {
    origin: egui::Pos2,
    scale: f32,
    /// 論理的な画面が占める領域
    area: egui::Rect,
}

impl Layout {
    fn new(rect: egui::Rect, size: (f32, f32)) -> Layout {
        let scale = (rect.width() / size.0)
            .min(rect.height() / size.1)
            .max(0.01);
        let (w, h) = (size.0 * scale, size.1 * scale);
        let origin = egui::pos2(rect.center().x - w / 2.0, rect.center().y - h / 2.0);
        Layout {
            origin,
            scale,
            area: egui::Rect::from_min_size(origin, egui::vec2(w, h)),
        }
    }

    fn point(&self, x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(
            self.origin.x + x * self.scale,
            self.origin.y + y * self.scale,
        )
    }

    /// 画面の座標を論理座標にする
    fn logical(&self, p: egui::Pos2) -> (f32, f32) {
        (
            (p.x - self.origin.x) / self.scale,
            (p.y - self.origin.y) / self.scale,
        )
    }
}

fn color(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// egui のキーを、canvas のキーの名前（小文字）にする
fn key_name(key: egui::Key) -> Option<&'static str> {
    let debug = format!("{key:?}");
    let name = debug
        .strip_prefix("Arrow")
        .or_else(|| debug.strip_prefix("Num"))
        .unwrap_or(&debug)
        .to_ascii_lowercase();
    KEY_NAMES.iter().copied().find(|k| *k == name)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Ok(cmds) = self.frames.try_recv() {
            self.cmds = cmds;
            self.waiting = false;
        }
        if self.quit.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // 描いてから、同じ配置で入力を論理座標に直す
        let layout = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let layout = Layout::new(ui.max_rect(), self.size);
                ui.painter().rect_filled(
                    ui.max_rect(),
                    egui::CornerRadius::ZERO,
                    color(self.background),
                );
                paint(
                    &ui.painter_at(layout.area),
                    &layout,
                    &self.cmds,
                    self.background,
                    &mut self.images,
                );
                layout
            })
            .inner;

        self.collect_input(&ctx);
        let now = Instant::now();
        let due = self
            .last_sent
            .is_none_or(|t| now.duration_since(t) >= self.interval * 3 / 4);
        if !self.waiting && due {
            let dt = self
                .last_sent
                .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
            let input = self.snapshot(&ctx, &layout);
            self.last_sent = Some(now);
            self.waiting = true;
            let _ = self
                .events
                .send(Event::Ext(Box::new(CanvasMsg::Frame { dt, input })));
        }
        // 次のフレームの時刻まで待って、起きる（命令の列が届いたときは、届いた側が起こす）
        let wait = self.last_sent.map_or(Duration::ZERO, |t| {
            self.interval
                .saturating_sub(Instant::now().duration_since(t))
        });
        ctx.request_repaint_after(wait.max(Duration::from_millis(1)));
    }
}

impl App {
    /// このフレームに起きたキー・ボタン・ホイールを、次の送信までためる
    fn collect_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    ..
                } = event
                    && let Some(name) = key_name(*key)
                {
                    let set = if *pressed {
                        &mut self.pressed
                    } else {
                        &mut self.released
                    };
                    set.insert(name.to_string());
                }
            }
            for (n, button) in [
                egui::PointerButton::Primary,
                egui::PointerButton::Secondary,
                egui::PointerButton::Middle,
            ]
            .into_iter()
            .enumerate()
            {
                if i.pointer.button_pressed(button) {
                    self.buttons_pressed[n] = true;
                }
            }
            self.wheel += i.smooth_scroll_delta.y;
        });
    }

    /// 送るときの入力の状態を作り、ためたものを空にする
    fn snapshot(&mut self, ctx: &egui::Context, layout: &Layout) -> Input {
        let mut input = ctx.input(|i| {
            let mut down: HashSet<String> = i
                .keys_down
                .iter()
                .filter_map(|k| key_name(*k))
                .map(String::from)
                .collect();
            for (on, name) in [
                (i.modifiers.shift, "shift"),
                (i.modifiers.ctrl, "ctrl"),
                (i.modifiers.alt, "alt"),
            ] {
                if on {
                    down.insert(name.to_string());
                }
            }
            let pos = i.pointer.hover_pos().or_else(|| i.pointer.latest_pos());
            let (mouse_x, mouse_y) = pos.map_or((0.0, 0.0), |p| layout.logical(p));
            Input {
                down,
                mouse_x,
                mouse_y,
                inside: pos.is_some_and(|p| layout.area.contains(p)),
                buttons: [
                    i.pointer.button_down(egui::PointerButton::Primary),
                    i.pointer.button_down(egui::PointerButton::Secondary),
                    i.pointer.button_down(egui::PointerButton::Middle),
                ],
                ..Input::default()
            }
        });
        input.pressed = std::mem::take(&mut self.pressed);
        input.released = std::mem::take(&mut self.released);
        input.buttons_pressed = std::mem::take(&mut self.buttons_pressed);
        input.wheel = std::mem::take(&mut self.wheel);
        input
    }
}

/// 読めた画像（テクスチャと、ピクセルの大きさ）。読めなかったものは None（もう警告を出した）
type Loaded = Option<(egui::TextureHandle, [f32; 2])>;

/// 読み込んだ画像（GPU のテクスチャ）
#[derive(Default)]
struct Images {
    textures: HashMap<(String, bool), Loaded>,
}

impl Images {
    /// 画像を（初めてなら）読み込む。`smooth` は拡大・縮小の補間
    fn get(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        smooth: bool,
    ) -> Option<&(egui::TextureHandle, [f32; 2])> {
        self.textures
            .entry((path.to_string(), smooth))
            .or_insert_with(|| match image::open(path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let options = if smooth {
                        egui::TextureOptions::LINEAR
                    } else {
                        egui::TextureOptions::NEAREST
                    };
                    let handle = ctx.load_texture(path, color, options);
                    Some((handle, [size[0] as f32, size[1] as f32]))
                }
                Err(e) => {
                    eprintln!("canvas: 画像 `{path}` を読めません: {e}");
                    None
                }
            })
            .as_ref()
    }
}

fn paint(
    painter: &egui::Painter,
    layout: &Layout,
    cmds: &[Cmd],
    background: Color,
    images: &mut Images,
) {
    let scale = layout.scale;
    // 論理的な画面は、まず背景色で塗る
    painter.rect_filled(layout.area, egui::CornerRadius::ZERO, color(background));
    for cmd in cmds {
        match cmd {
            Cmd::Clear(c) => {
                painter.rect_filled(layout.area, egui::CornerRadius::ZERO, color(*c));
            }
            Cmd::Rect {
                x,
                y,
                w,
                h,
                color: c,
                stroke,
            } => {
                let rect = egui::Rect::from_min_size(
                    layout.point(*x, *y),
                    egui::vec2(w * scale, h * scale),
                );
                match stroke {
                    Some(width) => {
                        painter.rect_stroke(
                            rect,
                            egui::CornerRadius::ZERO,
                            egui::Stroke::new(width * scale, color(*c)),
                            egui::StrokeKind::Middle,
                        );
                    }
                    None => {
                        painter.rect_filled(rect, egui::CornerRadius::ZERO, color(*c));
                    }
                }
            }
            Cmd::Circle {
                x,
                y,
                r,
                color: c,
                stroke,
            } => {
                let center = layout.point(*x, *y);
                match stroke {
                    Some(width) => {
                        painter.circle_stroke(
                            center,
                            r * scale,
                            egui::Stroke::new(width * scale, color(*c)),
                        );
                    }
                    None => {
                        painter.circle_filled(center, r * scale, color(*c));
                    }
                }
            }
            Cmd::Line {
                x1,
                y1,
                x2,
                y2,
                color: c,
                width,
            } => {
                painter.line_segment(
                    [layout.point(*x1, *y1), layout.point(*x2, *y2)],
                    egui::Stroke::new(width * scale, color(*c)),
                );
            }
            Cmd::Polygon {
                points,
                color: c,
                stroke,
            } => {
                let points: Vec<egui::Pos2> =
                    points.iter().map(|(x, y)| layout.point(*x, *y)).collect();
                let shape = match stroke {
                    Some(width) => egui::Shape::closed_line(
                        points,
                        egui::Stroke::new(width * scale, color(*c)),
                    ),
                    None => egui::Shape::convex_polygon(points, color(*c), egui::Stroke::NONE),
                };
                painter.add(shape);
            }
            Cmd::Image {
                path,
                x,
                y,
                w,
                h,
                src,
                tint,
                flip_x,
                flip_y,
                smooth,
            } => {
                let Some((texture, size)) = images.get(painter.ctx(), path, *smooth) else {
                    continue;
                };
                // 描く範囲（画像のピクセル）→ 0〜1 の UV
                let [sx, sy, sw, sh] = src.unwrap_or([0.0, 0.0, size[0], size[1]]);
                let (mut u0, mut u1) = (sx / size[0], (sx + sw) / size[0]);
                let (mut v0, mut v1) = (sy / size[1], (sy + sh) / size[1]);
                if *flip_x {
                    std::mem::swap(&mut u0, &mut u1);
                }
                if *flip_y {
                    std::mem::swap(&mut v0, &mut v1);
                }
                let (width, height) = (w.unwrap_or(sw), h.unwrap_or(sh));
                let rect = egui::Rect::from_min_size(
                    layout.point(*x, *y),
                    egui::vec2(width * scale, height * scale),
                );
                let uv = egui::Rect::from_min_max(egui::pos2(u0, v0), egui::pos2(u1, v1));
                painter.image(texture.id(), rect, uv, color(*tint));
            }
            Cmd::Text {
                x,
                y,
                text,
                color: c,
                size,
            } => {
                painter.text(
                    layout.point(*x, *y),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(size * scale),
                    color(*c),
                );
            }
        }
    }
}
