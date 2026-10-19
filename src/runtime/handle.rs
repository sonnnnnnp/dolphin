//! ハンドル: 閉じる必要のある外部の資源（TCP の接続・待ち受けなど。design.md 5.21）

use std::cell::RefCell;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Duration;

/// WebSocket のサーバー側の接続に送る指示（送信は補助スレッドが行う。design.md 5.22）
#[derive(Debug)]
pub enum WsOut {
    Text(String),
    Binary(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String),
}

/// WebSocket のクライアントの回線。普通の TCP か、TLS（`wss://`、design.md 5.35）。
/// 読む側と書く側で別々に持てるよう、複製できる（TLS は同じ接続を共有する）
pub enum WsConn {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Rc<RefCell<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>>),
}

impl WsConn {
    pub fn try_clone(&self) -> io::Result<WsConn> {
        match self {
            WsConn::Plain(s) => Ok(WsConn::Plain(s.try_clone()?)),
            #[cfg(feature = "tls")]
            WsConn::Tls(s) => Ok(WsConn::Tls(Rc::clone(s))),
        }
    }

    fn with_socket<T>(&self, f: impl FnOnce(&TcpStream) -> T) -> T {
        match self {
            WsConn::Plain(s) => f(s),
            #[cfg(feature = "tls")]
            WsConn::Tls(s) => f(&s.borrow().sock),
        }
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.with_socket(TcpStream::peer_addr)
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.with_socket(TcpStream::local_addr)
    }

    pub fn set_nodelay(&self, on: bool) -> io::Result<()> {
        self.with_socket(|s| s.set_nodelay(on))
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.with_socket(|s| s.set_read_timeout(timeout))
    }

    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.with_socket(|s| s.set_write_timeout(timeout))
    }

    /// TLS の close_notify を送る（送れなくてもよい）。TCP なら何もしない
    pub fn close_notify(&mut self) {
        #[cfg(feature = "tls")]
        if let WsConn::Tls(s) = self {
            let mut s = s.borrow_mut();
            s.conn.send_close_notify();
            let _ = s.flush();
        }
    }
}

impl Read for WsConn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            WsConn::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            WsConn::Tls(s) => s.borrow_mut().read(buf),
        }
    }
}

impl Write for WsConn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            WsConn::Plain(s) => s.write(buf),
            #[cfg(feature = "tls")]
            WsConn::Tls(s) => s.borrow_mut().write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            WsConn::Plain(s) => s.flush(),
            #[cfg(feature = "tls")]
            WsConn::Tls(s) => s.borrow_mut().flush(),
        }
    }
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
        reader: BufReader<WsConn>,
        writer: WsConn,
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
    /// INTEGER を整数（`int`）で返すか（design.md 5.40）。false なら数値
    pub ints: bool,
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
