//! ハンドル: 閉じる必要のある外部の資源（TCP の接続・待ち受けなど。design.md 5.21）

use std::cell::RefCell;
use std::fmt;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::mpsc::Sender;

/// WebSocket のサーバー側の接続に送る指示（送信は補助スレッドが行う。design.md 5.22）
#[derive(Debug)]
pub enum WsOut {
    Text(String),
    Binary(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String),
}

pub enum HandleKind {
    /// 読み込みはバッファ付き。書き込みは同じソケットの複製
    TcpConn {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    },
    TcpListener(TcpListener),
    /// WebSocket のクライアント側の接続（`std:ws` の `connect`）
    WsClient {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    },
    /// `http.serve` が WebSocket にした接続。送信は補助スレッドに頼む
    WsServer {
        tx: Sender<WsOut>,
    },
    /// SQLite の接続（`std:sqlite`。design.md 5.28）
    #[cfg(feature = "sqlite")]
    Sqlite(SqliteConn),
    #[cfg(feature = "sqlite")]
    SqliteClosed,
    Closed,
}

#[cfg(feature = "sqlite")]
pub struct SqliteConn {
    pub conn: rusqlite::Connection,
    /// 開いたときのパス（表示用。メモリ上なら `:memory:`）
    pub path: String,
}

pub struct Handle {
    pub inner: RefCell<HandleKind>,
}

impl Handle {
    pub fn new(kind: HandleKind) -> Rc<Self> {
        Rc::new(Self {
            inner: RefCell::new(kind),
        })
    }

    pub fn close(&self) {
        let mut inner = self.inner.borrow_mut();
        *inner = match &*inner {
            #[cfg(feature = "sqlite")]
            HandleKind::Sqlite(_) | HandleKind::SqliteClosed => HandleKind::SqliteClosed,
            _ => HandleKind::Closed,
        };
    }
}

/// `<tcp 接続 127.0.0.1:80>` のような表示
impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.inner.borrow() {
            HandleKind::TcpConn { writer, .. } => match writer.peer_addr() {
                Ok(addr) => write!(f, "<tcp 接続 {addr}>"),
                Err(_) => write!(f, "<tcp 接続>"),
            },
            HandleKind::TcpListener(listener) => match listener.local_addr() {
                Ok(addr) => write!(f, "<tcp 待ち受け {addr}>"),
                Err(_) => write!(f, "<tcp 待ち受け>"),
            },
            HandleKind::WsClient { writer, .. } => match writer.peer_addr() {
                Ok(addr) => write!(f, "<websocket 接続 {addr}>"),
                Err(_) => write!(f, "<websocket 接続>"),
            },
            HandleKind::WsServer { .. } => write!(f, "<websocket サーバー側の接続>"),
            #[cfg(feature = "sqlite")]
            HandleKind::Sqlite(db) => write!(f, "<sqlite 接続 {}>", db.path),
            #[cfg(feature = "sqlite")]
            HandleKind::SqliteClosed => write!(f, "<sqlite 閉じています>"),
            HandleKind::Closed => write!(f, "<tcp 閉じています>"),
        }
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({self})")
    }
}
