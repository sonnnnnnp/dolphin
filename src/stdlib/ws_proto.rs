//! WebSocket のプロトコル（RFC 6455。design.md 5.22）。フレームの読み書き、メッセージの組み立て、
//! ハンドシェイクの計算。ソケットには触れず、`Read` / `Write` だけを使う

use std::io::{self, Read, Write};

use super::encoding::base64_encode;

/// メッセージの大きさの上限（バイト）
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub const OP_CONTINUATION: u8 = 0;
pub const OP_TEXT: u8 = 1;
pub const OP_BINARY: u8 = 2;
pub const OP_CLOSE: u8 = 8;
pub const OP_PING: u8 = 9;
pub const OP_PONG: u8 = 10;

#[derive(Debug)]
pub enum ProtoError {
    Io(io::Error),
    /// RFC に反するフレーム（close コード 1002）
    Protocol(&'static str),
    /// テキストが UTF-8 でない（1007）
    BadUtf8,
    /// メッセージが大きすぎる（1009）
    TooLarge,
}

impl ProtoError {
    /// 相手に送る close コード
    pub fn close_code(&self) -> u16 {
        match self {
            ProtoError::Io(_) => 1006,
            ProtoError::Protocol(_) => 1002,
            ProtoError::BadUtf8 => 1007,
            ProtoError::TooLarge => 1009,
        }
    }
}

impl From<io::Error> for ProtoError {
    fn from(e: io::Error) -> Self {
        ProtoError::Io(e)
    }
}

#[derive(Debug, PartialEq)]
pub struct Frame {
    pub fin: bool,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// 組み立て済みのメッセージ
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    /// 終了コードと理由（本体がなければ None）
    Close(Option<(u16, String)>),
}

/// 1 フレームを読む。`expect_masked` は、サーバーが読む（クライアントのフレームはマスクされている）とき true
pub fn read_frame(r: &mut impl Read, expect_masked: bool) -> Result<Frame, ProtoError> {
    let mut head = [0u8; 2];
    r.read_exact(&mut head)?;
    let fin = head[0] & 0x80 != 0;
    if head[0] & 0x70 != 0 {
        return Err(ProtoError::Protocol("予約ビットが立っています"));
    }
    let opcode = head[0] & 0x0F;
    if !matches!(opcode, 0..=2 | 8..=10) {
        return Err(ProtoError::Protocol("知らないオペコードです"));
    }
    let masked = head[1] & 0x80 != 0;
    if masked != expect_masked {
        return Err(ProtoError::Protocol(if expect_masked {
            "クライアントのフレームがマスクされていません"
        } else {
            "サーバーのフレームがマスクされています"
        }));
    }
    let len = match head[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            let n = u64::from_be_bytes(b);
            if n >> 63 != 0 {
                return Err(ProtoError::Protocol("長さの最上位ビットが立っています"));
            }
            n
        }
        n => u64::from(n),
    };
    if opcode >= 8 && (!fin || len > 125) {
        return Err(ProtoError::Protocol(
            "制御フレームが断片化か 125 バイト超です",
        ));
    }
    if len > MAX_MESSAGE as u64 {
        return Err(ProtoError::TooLarge);
    }
    let mut key = [0u8; 4];
    if masked {
        r.read_exact(&mut key)?;
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= key[i % 4];
        }
    }
    Ok(Frame {
        fin,
        opcode,
        payload,
    })
}

/// 1 フレームを書く。`mask` があれば（クライアントのフレーム）マスクする
pub fn write_frame(
    w: &mut impl Write,
    opcode: u8,
    payload: &[u8],
    mask: Option<[u8; 4]>,
) -> io::Result<()> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => out.push(mask_bit | n as u8),
        n if n <= 0xFFFF => {
            out.push(mask_bit | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(mask_bit | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            out.extend_from_slice(&key);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        }
        None => out.extend_from_slice(payload),
    }
    w.write_all(&out)?;
    w.flush()
}

/// close フレームの本体（終了コードと理由）
pub fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    // 制御フレームの本体は 125 バイトまで
    let mut reason = reason;
    while reason.len() > 123 {
        let mut end = reason.len() - 1;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        reason = &reason[..end];
    }
    payload.extend_from_slice(reason.as_bytes());
    payload
}

fn parse_close(payload: &[u8]) -> Result<Option<(u16, String)>, ProtoError> {
    match payload.len() {
        0 => Ok(None),
        1 => Err(ProtoError::Protocol("close の本体が 1 バイトです")),
        _ => {
            let code = u16::from_be_bytes([payload[0], payload[1]]);
            let reason =
                String::from_utf8(payload[2..].to_vec()).map_err(|_| ProtoError::BadUtf8)?;
            Ok(Some((code, reason)))
        }
    }
}

/// 次のメッセージを読む。断片化は組み立てる。ping は `on_ping`（pong を返す処理）に渡し、
/// pong は読み捨てる
pub fn read_message(
    r: &mut impl Read,
    expect_masked: bool,
    mut on_ping: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<Message, ProtoError> {
    // 組み立て中のデータメッセージ（オペコードと、ここまでの本体）
    let mut partial: Option<(u8, Vec<u8>)> = None;
    loop {
        let frame = read_frame(r, expect_masked)?;
        match frame.opcode {
            OP_PING => on_ping(&frame.payload)?,
            OP_PONG => {}
            OP_CLOSE => return Ok(Message::Close(parse_close(&frame.payload)?)),
            OP_TEXT | OP_BINARY => {
                if partial.is_some() {
                    return Err(ProtoError::Protocol(
                        "断片化の途中で新しいメッセージが始まりました",
                    ));
                }
                if frame.fin {
                    return finish(frame.opcode, frame.payload);
                }
                partial = Some((frame.opcode, frame.payload));
            }
            _ => {
                let Some((opcode, data)) = partial.as_mut() else {
                    return Err(ProtoError::Protocol("続きのフレームだけが届きました"));
                };
                if data.len() + frame.payload.len() > MAX_MESSAGE {
                    return Err(ProtoError::TooLarge);
                }
                data.extend_from_slice(&frame.payload);
                if frame.fin {
                    let opcode = *opcode;
                    let data = std::mem::take(data);
                    return finish(opcode, data);
                }
            }
        }
    }
}

fn finish(opcode: u8, data: Vec<u8>) -> Result<Message, ProtoError> {
    if opcode == OP_TEXT {
        String::from_utf8(data)
            .map(Message::Text)
            .map_err(|_| ProtoError::BadUtf8)
    } else {
        Ok(Message::Binary(data))
    }
}

// ---- ハンドシェイク ----

/// `Sec-WebSocket-Accept` の値
pub fn accept_key(key: &str) -> String {
    let mut input = key.trim().as_bytes().to_vec();
    input.extend_from_slice(GUID.as_bytes());
    base64_encode(&sha1(&input), false)
}

/// SHA-1（ハンドシェイクの計算だけに使う。暗号の用途には使わない）
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        for (hv, v) in h.iter_mut().zip([a, b, c, d, e]) {
            *hv = hv.wrapping_add(v);
        }
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&sha1(&vec![b'a'; 1000])),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }

    #[test]
    fn rfc_accept_key() {
        // RFC 6455 4.2.2 の例
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frame_roundtrip() {
        for (payload, mask) in [
            (b"hello".to_vec(), None),
            (b"hello".to_vec(), Some([1, 2, 3, 4])),
            (vec![7; 200], Some([9, 8, 7, 6])),
            (vec![1; 70000], None),
            (Vec::new(), Some([0, 0, 0, 0])),
        ] {
            let mut buf = Vec::new();
            write_frame(&mut buf, OP_BINARY, &payload, mask).unwrap();
            let frame = read_frame(&mut &buf[..], mask.is_some()).unwrap();
            assert_eq!(
                frame,
                Frame {
                    fin: true,
                    opcode: OP_BINARY,
                    payload
                }
            );
        }
    }

    #[test]
    fn rfc_example_frames() {
        // RFC 6455 5.7: マスクされた "Hello"
        let masked = [
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        let frame = read_frame(&mut &masked[..], true).unwrap();
        assert_eq!(frame.payload, b"Hello");
        // マスクなしの "Hello"
        let plain = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        assert_eq!(
            read_frame(&mut &plain[..], false).unwrap().payload,
            b"Hello"
        );
        // 期待と違うマスクの有無は、プロトコルの違反
        assert!(matches!(
            read_frame(&mut &plain[..], true),
            Err(ProtoError::Protocol(_))
        ));
        assert!(matches!(
            read_frame(&mut &masked[..], false),
            Err(ProtoError::Protocol(_))
        ));
    }

    #[test]
    fn invalid_frames() {
        // 予約ビット、知らないオペコード、断片化した ping、長すぎる ping
        for bytes in [
            vec![0xC1, 0x00],
            vec![0x83, 0x00],
            vec![0x09, 0x00],
            vec![0x89, 0x7E, 0x00, 0x7E],
        ] {
            assert!(
                matches!(
                    read_frame(&mut &bytes[..], false),
                    Err(ProtoError::Protocol(_))
                ),
                "{bytes:?}"
            );
        }
        // 途中で終わる
        assert!(matches!(
            read_frame(&mut &[0x81, 0x05, 0x48][..], false),
            Err(ProtoError::Io(_))
        ));
    }

    fn frames(parts: &[(bool, u8, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        for (fin, opcode, payload) in parts {
            let mut f = Vec::new();
            write_frame(&mut f, *opcode, payload, None).unwrap();
            if !fin {
                f[0] &= 0x7F;
            }
            buf.extend(f);
        }
        buf
    }

    #[test]
    fn fragmented_messages_with_ping() {
        let mut pings = Vec::new();
        let buf = frames(&[
            (false, OP_TEXT, "こん".as_bytes()),
            (true, OP_PING, b"p"),
            (false, OP_CONTINUATION, "にち".as_bytes()),
            (true, OP_CONTINUATION, "は".as_bytes()),
        ]);
        let message = read_message(&mut &buf[..], false, |p| {
            pings.push(p.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(message, Message::Text("こんにちは".into()));
        assert_eq!(pings, [b"p".to_vec()]);
    }

    #[test]
    fn message_errors() {
        let new_in_fragment = frames(&[(false, OP_TEXT, b"a"), (true, OP_TEXT, b"b")]);
        assert!(matches!(
            read_message(&mut &new_in_fragment[..], false, |_| Ok(())),
            Err(ProtoError::Protocol(_))
        ));
        let lone = frames(&[(true, OP_CONTINUATION, b"a")]);
        assert!(matches!(
            read_message(&mut &lone[..], false, |_| Ok(())),
            Err(ProtoError::Protocol(_))
        ));
        let bad_utf8 = frames(&[(true, OP_TEXT, &[0xFF, 0xFE])]);
        assert!(matches!(
            read_message(&mut &bad_utf8[..], false, |_| Ok(())),
            Err(ProtoError::BadUtf8)
        ));
        let binary = frames(&[(true, OP_BINARY, &[0xFF, 0xFE])]);
        assert_eq!(
            read_message(&mut &binary[..], false, |_| Ok(())).unwrap(),
            Message::Binary(vec![0xFF, 0xFE])
        );
    }

    #[test]
    fn close_messages() {
        let payload = close_payload(1000, "bye");
        let buf = frames(&[(true, OP_CLOSE, &payload)]);
        assert_eq!(
            read_message(&mut &buf[..], false, |_| Ok(())).unwrap(),
            Message::Close(Some((1000, "bye".into())))
        );
        let empty = frames(&[(true, OP_CLOSE, b"")]);
        assert_eq!(
            read_message(&mut &empty[..], false, |_| Ok(())).unwrap(),
            Message::Close(None)
        );
        let one = frames(&[(true, OP_CLOSE, b"x")]);
        assert!(read_message(&mut &one[..], false, |_| Ok(())).is_err());
        // 長い理由は 125 バイトに収める
        assert!(close_payload(1000, &"あ".repeat(100)).len() <= 125);
    }
}
