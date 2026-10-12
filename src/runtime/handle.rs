//! ハンドル: 閉じる必要のある外部の資源（TCP の接続・待ち受けなど。design.md 5.21）

use std::cell::RefCell;
use std::fmt;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;

pub enum HandleKind {
    /// 読み込みはバッファ付き。書き込みは同じソケットの複製
    TcpConn {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    },
    TcpListener(TcpListener),
    Closed,
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
        *self.inner.borrow_mut() = HandleKind::Closed;
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
            HandleKind::Closed => write!(f, "<tcp 閉じています>"),
        }
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({self})")
    }
}
