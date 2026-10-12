//! メインスレッドのホスト（design.md 5.24）。GUI（egui）はメインスレッドで動かす必要があるので、
//! `dolphin` コマンドのメインスレッドは、インタプリタのスレッドが終わるまで、
//! ウィンドウを開く依頼を待ち受ける

use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use std::sync::atomic::{AtomicBool, Ordering};

use super::WindowRequest;
use super::canvas::CanvasRequest;

/// ホストに渡す依頼
pub enum HostRequest {
    Ui(WindowRequest),
    Canvas(CanvasRequest),
}

static HOST: OnceLock<Sender<HostRequest>> = OnceLock::new();
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// ウィンドウを開く権利を取る。1 プロセスに 1 回だけ true（ウィンドウのループを作り直せない
/// プラットフォームがあるため）
pub(super) fn claim() -> bool {
    !CLAIMED.swap(true, Ordering::SeqCst)
}

/// ウィンドウを開く依頼の受け口
pub struct Host {
    requests: Receiver<HostRequest>,
}

/// メインスレッドで、インタプリタのスレッドを起こす前に呼ぶ
pub fn install() -> Host {
    let (tx, requests) = mpsc::channel();
    let _ = HOST.set(tx);
    Host { requests }
}

/// インタプリタのスレッドから、依頼を出す口（`install` していなければ None）
pub(super) fn sender() -> Option<Sender<HostRequest>> {
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

fn open_window(request: HostRequest) {
    #[cfg(feature = "ui")]
    match request {
        HostRequest::Ui(request) => super::gui::run_window(request),
        HostRequest::Canvas(request) => super::canvas_gui::run_window(request),
    }
    #[cfg(not(feature = "ui"))]
    {
        use crate::runtime::events::Event;
        // GUI のない版では、ウィンドウを開けないので、すぐ閉じたことにする
        let (events, msg): (_, Box<dyn std::any::Any + Send>) = match request {
            HostRequest::Ui(r) => (r.events, Box::new(super::UiMsg::Closed)),
            HostRequest::Canvas(r) => (r.events, Box::new(super::canvas::CanvasMsg::Closed)),
        };
        let _ = events.send(Event::Ext(msg));
    }
}
