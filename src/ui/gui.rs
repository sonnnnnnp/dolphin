//! egui（eframe）での描画（design.md 5.24）。メインスレッドで動く

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;

use super::{Node, UiMsg, WindowRequest};
use crate::runtime::data::Data;
use crate::runtime::events::Event;

/// ウィンドウを開いて、閉じられるまで戻らない。閉じたら、インタプリタに知らせる
pub fn run_window(request: WindowRequest) {
    let WindowRequest {
        options,
        trees,
        events,
        repaint,
        quit,
    } = request;
    let viewport = egui::ViewportBuilder::default()
        .with_title(options.title.clone())
        .with_inner_size([options.width, options.height]);
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let app_events = events.clone();
    let font = options.font.clone();
    let result = eframe::run_native(
        &options.title,
        native,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx, font.as_deref());
            let ctx = cc.egui_ctx.clone();
            if let Ok(mut slot) = repaint.lock() {
                *slot = Some(Box::new(move || ctx.request_repaint()));
            }
            Ok(Box::new(App {
                trees,
                events: app_events,
                quit,
                tree: None,
                edits: HashMap::new(),
            }))
        }),
    );
    if let Err(e) = result {
        eprintln!("ui: ウィンドウを開けません: {e}");
    }
    let _ = events.send(Event::Ext(Box::new(UiMsg::Closed)));
}

struct App {
    trees: Receiver<Node>,
    events: Sender<Event>,
    quit: Arc<AtomicBool>,
    tree: Option<Node>,
    /// 画面が届くまでの間、入力中の文字列を表示するための控え（部品の番号 → 文字列）
    edits: HashMap<u64, String>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 届いた画面のうち、最新のものを使う
        while let Ok(tree) = self.trees.try_recv() {
            self.tree = Some(tree);
            self.edits.clear();
        }
        if self.quit.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let mut outbox = Vec::new();
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(tree) = &self.tree {
                draw(tree, ui, &mut self.edits, &mut outbox);
            }
        });
        for msg in outbox {
            let _ = self.events.send(Event::Ext(Box::new(msg)));
        }
    }
}

fn callback(id: u64, arg: Option<Data>) -> UiMsg {
    UiMsg::Callback { id, arg }
}

fn draw(node: &Node, ui: &mut egui::Ui, edits: &mut HashMap<u64, String>, out: &mut Vec<UiMsg>) {
    match node {
        Node::Text {
            text,
            size,
            color,
            bold,
        } => {
            let mut rich = egui::RichText::new(text);
            if let Some(size) = size {
                rich = rich.size(*size);
            }
            if let Some([r, g, b]) = color {
                rich = rich.color(egui::Color32::from_rgb(*r, *g, *b));
            }
            if *bold {
                rich = rich.strong();
            }
            ui.label(rich);
        }
        Node::Button { label, id, enabled } => {
            if ui.add_enabled(*enabled, egui::Button::new(label)).clicked() {
                out.push(callback(*id, None));
            }
        }
        Node::TextInput {
            value,
            id,
            hint,
            multiline,
            password,
        } => {
            let mut buffer = edits.get(id).cloned().unwrap_or_else(|| value.clone());
            let response = if *multiline {
                ui.add(egui::TextEdit::multiline(&mut buffer).hint_text(hint.as_str()))
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut buffer)
                        .hint_text(hint.as_str())
                        .password(*password),
                )
            };
            if response.changed() {
                edits.insert(*id, buffer.clone());
                out.push(callback(*id, Some(Data::Str(buffer))));
            }
        }
        Node::Checkbox { label, checked, id } => {
            let mut now = *checked;
            if ui.checkbox(&mut now, label).changed() {
                out.push(callback(*id, Some(Data::Bool(now))));
            }
        }
        Node::Slider {
            value,
            min,
            max,
            id,
        } => {
            let mut now = *value;
            if ui.add(egui::Slider::new(&mut now, *min..=*max)).changed() {
                out.push(callback(*id, Some(Data::Num(now))));
            }
        }
        Node::Select { labels, index, id } => {
            let selected = labels.get(*index).cloned().unwrap_or_default();
            egui::ComboBox::from_id_salt(*id)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (i, label) in labels.iter().enumerate() {
                        if ui.selectable_label(i == *index, label).clicked() && i != *index {
                            out.push(callback(*id, Some(Data::Num(i as f64))));
                        }
                    }
                });
        }
        Node::Progress(fraction) => {
            ui.add(egui::ProgressBar::new(*fraction));
        }
        Node::Column { children, spacing } => {
            ui.vertical(|ui| {
                if let Some(spacing) = spacing {
                    ui.spacing_mut().item_spacing.y = *spacing;
                }
                for child in children {
                    draw(child, ui, edits, out);
                }
            });
        }
        Node::Row { children, spacing } => {
            ui.horizontal(|ui| {
                if let Some(spacing) = spacing {
                    ui.spacing_mut().item_spacing.x = *spacing;
                }
                for child in children {
                    draw(child, ui, edits, out);
                }
            });
        }
        Node::Group(children) => {
            ui.group(|ui| {
                for child in children {
                    draw(child, ui, edits, out);
                }
            });
        }
        Node::Scroll(child) => {
            egui::ScrollArea::vertical().show(ui, |ui| draw(child, ui, edits, out));
        }
        Node::Separator => {
            ui.separator();
        }
        Node::Space(size) => {
            ui.add_space(*size);
        }
    }
}

// ---- フォント ----

/// 日本語を表示できるシステムのフォント（`.ttc` は先頭のフォント）
const SYSTEM_FONTS: &[&str] = &[
    // Windows
    "C:\\Windows\\Fonts\\YuGothR.ttc",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    // macOS
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
];

fn setup_fonts(ctx: &egui::Context, option: Option<&str>) {
    let candidates = option.into_iter().chain(SYSTEM_FONTS.iter().copied());
    let Some(bytes) = candidates
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
    else {
        eprintln!(
            "ui: 日本語のフォントが見つかりません（日本語は表示できません）。`ui.run` の `font` でフォントのファイルを指定してください"
        );
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_string(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_string());
    }
    ctx.set_fonts(fonts);
}
