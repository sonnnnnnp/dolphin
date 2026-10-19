//! ワーカー（design.md 5.43）。別のスレッドに別のインタプリタを立てて、メッセージ（`Data`）で
//! やり取りする。ここには、ワーカーの中のインタプリタが持つ口と、親が持つハンドルの実体を置く

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};

use super::data::Data;
use super::events::{Event, StreamEvent};

/// ワーカーから親へ届くもの（ブロッキング型）
pub enum ChildMsg {
    Message(Data),
    /// 終わった。終了コードと、実行時エラーの表示（なければ空）。これが最後
    Exit(i32, String),
}

/// ワーカーが親へ送る口
#[derive(Clone)]
pub enum WorkerOut {
    /// コールバック型。親のイベントループへ送る
    Events { tx: Sender<Event>, id: u64 },
    /// ブロッキング型。親の `recv` `join` が受け取る
    Channel(Sender<ChildMsg>),
}

impl WorkerOut {
    /// 親へメッセージを送る。親がいなくなっていれば false
    pub fn post(&self, data: Data) -> bool {
        match self {
            WorkerOut::Events { tx, id } => tx
                .send(Event::Stream {
                    id: *id,
                    event: StreamEvent::Message(data),
                })
                .is_ok(),
            WorkerOut::Channel(tx) => tx.send(ChildMsg::Message(data)).is_ok(),
        }
    }

    /// 終わりを知らせる（最後に 1 回だけ）
    pub fn exit(&self, code: i32, reason: String) {
        match self {
            WorkerOut::Events { tx, id } => {
                let code = u16::try_from(code).unwrap_or(1);
                let _ = tx.send(Event::Stream {
                    id: *id,
                    event: StreamEvent::Close(code, reason),
                });
            }
            WorkerOut::Channel(tx) => {
                let _ = tx.send(ChildMsg::Exit(code, reason));
            }
        }
    }
}

/// ワーカーの中のインタプリタが持つもの
pub struct WorkerContext {
    pub out: WorkerOut,
    /// 親からのメッセージ。親が `terminate` したか、いなくなると切れる
    pub inbox: Receiver<Data>,
    pub arg: Data,
}

/// 親が持つワーカーのハンドルの実体
pub struct WorkerHandle {
    /// 表示用（`spawn` に渡したパス）
    pub label: String,
    /// ワーカーへ送る口。`terminate` で捨てる
    pub to_child: Option<Sender<Data>>,
    /// ブロッキング型で、ワーカーから受け取る口。コールバック型では None
    pub from_child: Option<Receiver<ChildMsg>>,
    /// `join` の間などに先に届いたメッセージ
    pub queue: VecDeque<Data>,
    /// 終わったあとの（終了コード、実行時エラーの表示）
    pub finished: Option<(i32, String)>,
    /// コールバック型の登録番号
    pub event_id: Option<u64>,
}
