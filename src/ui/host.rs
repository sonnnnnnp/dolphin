//! メインスレッドのホスト（design.md 5.24）。GUI（egui）はメインスレッドで動かす必要があるので、
//! `dolphin` コマンドのメインスレッドは、インタプリタのスレッドが終わるまで、
//! ウィンドウを開く依頼を待ち受ける

use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use super::WindowRequest;

static HOST: OnceLock<Sender<WindowRequest>> = OnceLock::new();

/// ウィンドウを開く依頼の受け口
pub struct Host {
    requests: Receiver<WindowRequest>,
}

/// メインスレッドで、インタプリタのスレッドを起こす前に呼ぶ
pub fn install() -> Host {
    let (tx, requests) = mpsc::channel();
    let _ = HOST.set(tx);
    Host { requests }
}

/// インタプリタのスレッドから、依頼を出す口（`install` していなければ None）
pub(super) fn sender() -> Option<Sender<WindowRequest>> {
    HOST.get().cloned()
}

/// インタプリタのスレッドが終わるまで、依頼を処理する。インタプリタのスレッドの結果（終了コード）を返す
pub fn run(host: Host, interpreter: JoinHandle<i32>) -> i32 {
    loop {
        match host.requests.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => open_window(request),
            Err(RecvTimeoutError::Timeout) => {
                if interpreter.is_finished() {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    // パニックした場合、メッセージはすでに表示されている
    interpreter.join().unwrap_or(101)
}

fn open_window(request: WindowRequest) {
    #[cfg(feature = "ui")]
    super::gui::run_window(request);
    #[cfg(not(feature = "ui"))]
    {
        use crate::runtime::events::Event;
        let _ = request
            .events
            .send(Event::Ext(Box::new(super::UiMsg::Closed)));
    }
}
