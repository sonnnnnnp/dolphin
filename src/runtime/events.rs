//! イベントループ（design.md 5.23）
//!
//! 補助スレッドで進めた処理の結果は `Event` としてキューに届く。インタプリタのスレッドが、
//! 届いた順にコールバックを 1 つずつ呼ぶ

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use super::data::Data;
use super::error::RuntimeError;
use super::interp::Interpreter;
use super::value::Value;
use crate::diag::Diagnostic;

/// 待っていられる処理の数の上限
pub const MAX_PENDING: usize = 10_000;

/// 補助スレッドからインタプリタのスレッドへ届くもの
pub enum Event {
    /// 非同期の関数の結果。`id` のコールバックに `data` を渡す
    Done { id: u64, data: Data },
    /// タイマーの時刻。`id` のコールバックを引数なしで呼ぶ
    Tick { id: u64 },
    /// ほかの仕組み（`http.serve` など）が自分のために送るもの
    Ext(Box<dyn Any + Send>),
}

/// `Event::Ext` を受け取る処理（1 つだけ登録できる）。`Err` は `os.exit` などで全体を止める合図
pub type ExtHandler =
    Box<dyn FnMut(&mut Interpreter<'_>, Box<dyn Any + Send>) -> Result<(), String>>;

struct Pending {
    callback: Value,
    /// タイマーなら、止める旗と、繰り返すか
    timer: Option<(Arc<AtomicBool>, bool)>,
}

pub struct Events {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    ext: Option<ExtHandler>,
    /// `Ext` を受ける仕組みが動いている間は、待っているものがなくならない
    ext_active: bool,
    running: bool,
    stopped: bool,
}

impl Default for Events {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            next_id: 1,
            pending: HashMap::new(),
            ext: None,
            ext_active: false,
            running: false,
            stopped: false,
        }
    }
}

impl Events {
    /// 補助スレッドに渡す送り口
    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    /// 待っている処理があるか（イベントループを動かす必要があるか）
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.ext_active
    }

    /// `Ext` の受け手を登録する。すでにあれば Err
    pub fn set_ext(&mut self, handler: ExtHandler) -> Result<(), &'static str> {
        if self.ext_active {
            return Err("すでに別のサーバーが動いています");
        }
        self.ext = Some(handler);
        self.ext_active = true;
        Ok(())
    }

    pub fn clear_ext(&mut self) {
        self.ext = None;
        self.ext_active = false;
    }

    /// コールバックを登録して、番号を返す。`timer` は止める旗と繰り返すか
    pub fn register(
        &mut self,
        callback: Value,
        timer: Option<(Arc<AtomicBool>, bool)>,
    ) -> Result<u64, String> {
        if self.pending.len() >= MAX_PENDING {
            return Err(format!(
                "待っている非同期の処理が多すぎます（{MAX_PENDING} 個まで）"
            ));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, Pending { callback, timer });
        Ok(id)
    }

    /// タイマーを止める / 結果を捨てる
    pub fn cancel(&mut self, id: u64) {
        if let Some(Pending {
            timer: Some((flag, _)),
            ..
        }) = self.pending.remove(&id)
        {
            flag.store(true, Ordering::SeqCst);
        }
    }

    pub fn stop(&mut self) {
        self.stopped = true;
    }
}

impl Interpreter<'_> {
    /// 待っているものがなくなるまで（`stop` まで）、届いたイベントを順に処理する。
    /// `Err` は `os.exit`（`exit_code` が入っている）
    pub fn run_event_loop(&mut self) -> Result<(), String> {
        if self.events.running {
            return Err("イベントループの中では、もう一度動かせません".into());
        }
        self.events.running = true;
        self.events.stopped = false;
        let result = self.event_loop();
        self.events.running = false;
        result
    }

    fn event_loop(&mut self) -> Result<(), String> {
        while !self.events.stopped && self.events.has_pending() {
            // 送り口を自分も持っているので、切れることはない
            let Ok(event) = self.events.rx.recv() else {
                return Ok(());
            };
            match event {
                Event::Done { id, data } => {
                    if let Some(pending) = self.events.pending.remove(&id) {
                        self.call_callback(&pending.callback, vec![data.into_value()])?;
                    }
                }
                Event::Tick { id } => {
                    let Some(pending) = self.events.pending.get(&id) else {
                        continue;
                    };
                    let Some((flag, repeat)) = &pending.timer else {
                        continue;
                    };
                    let (cancelled, repeat) = (flag.load(Ordering::SeqCst), *repeat);
                    let callback = pending.callback.clone();
                    if cancelled || !repeat {
                        self.events.pending.remove(&id);
                    }
                    if !cancelled {
                        self.call_callback(&callback, Vec::new())?;
                    }
                }
                Event::Ext(payload) => {
                    if let Some(mut handler) = self.events.ext.take() {
                        let result = handler(self, payload);
                        if self.events.ext_active {
                            self.events.ext = Some(handler);
                        }
                        result?;
                    }
                }
            }
        }
        Ok(())
    }

    /// コールバックを呼ぶ。エラーは標準エラーに出して続ける。`os.exit` だけは `Err`
    fn call_callback(&mut self, callback: &Value, args: Vec<Value>) -> Result<(), String> {
        if let Err(message) = self.call_value(callback, args) {
            if self.exit_code.is_some() {
                return Err(message);
            }
            match self.take_native_error() {
                Some(e) => eprintln!("{}", describe(e)),
                None => eprintln!("非同期のコールバックでエラー: {message}"),
            }
        }
        Ok(())
    }
}

fn describe(e: RuntimeError) -> String {
    let diag = Diagnostic::from(e);
    let mut text = format!(
        "非同期のコールバックでエラー: {}:{}: {}",
        diag.span.line, diag.span.col, diag.message
    );
    for note in &diag.notes {
        text.push_str(&format!("\n  {note}"));
    }
    text
}
