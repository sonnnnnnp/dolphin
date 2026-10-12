//! `std:audio`: play, stop, stop_all, pause, resume, set_volume, set_master_volume, is_playing
//! （design.md 5.27）。`rodio` を使う（`audio` feature）
//!
//! 再生は補助スレッドが受け持つ（出力機器は、そのスレッドが開いて持つ）。インタプリタのスレッドは、
//! ファイルを開いて形式を確かめ（失敗をすぐ返すため）、デコーダーを補助スレッドに渡す

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Player, Source};

use super::{error_value, expect_args, int_arg, num_arg, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

type BoxSource = Box<dyn Source + Send>;

enum Cmd {
    Play {
        id: u64,
        source: BoxSource,
        volume: f32,
        speed: f32,
        finished: Arc<AtomicBool>,
    },
    Stop(u64),
    StopAll,
    SetVolume(u64, f32),
    Pause(u64),
    Resume(u64),
    Master(f32),
}

struct Audio {
    tx: Sender<Cmd>,
    /// 音の番号 → 終わったか（補助スレッドが、終わりを見つけたら立てる）
    sounds: HashMap<u64, Arc<AtomicBool>>,
    next_id: u64,
}

thread_local! {
    static AUDIO: RefCell<Option<Audio>> = const { RefCell::new(None) };
}

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:audio",
        &[
            ("audio.play", play),
            ("audio.stop", |_, a| simple("audio.stop", a, Cmd::Stop)),
            ("audio.stop_all", stop_all),
            ("audio.pause", |_, a| simple("audio.pause", a, Cmd::Pause)),
            ("audio.resume", |_, a| {
                simple("audio.resume", a, Cmd::Resume)
            }),
            ("audio.set_volume", set_volume),
            ("audio.set_master_volume", set_master_volume),
            ("audio.is_playing", is_playing),
        ],
    );
}

// ---- 補助スレッド ----

struct Playing {
    player: Player,
    volume: f32,
    finished: Arc<AtomicBool>,
}

/// 出力機器を開いて、指示を処理し続ける（`Sender` が全部なくなるまで）
fn audio_thread(rx: &Receiver<Cmd>, ready: &Sender<Result<(), String>>) {
    let mut sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    // プログラムが終わるときの「音が止まります」という表示は要らない
    sink.log_on_drop(false);
    let _ = ready.send(Ok(()));
    let mut playing: HashMap<u64, Playing> = HashMap::new();
    let mut master = 1.0f32;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(cmd) => match cmd {
                Cmd::Play {
                    id,
                    source,
                    volume,
                    speed,
                    finished,
                } => {
                    let player = Player::connect_new(sink.mixer());
                    player.set_volume(volume * master);
                    player.set_speed(speed);
                    player.append(source);
                    playing.insert(
                        id,
                        Playing {
                            player,
                            volume,
                            finished,
                        },
                    );
                }
                Cmd::Stop(id) => {
                    if let Some(p) = playing.remove(&id) {
                        p.player.stop();
                        p.finished.store(true, Ordering::SeqCst);
                    }
                }
                Cmd::StopAll => {
                    for (_, p) in playing.drain() {
                        p.player.stop();
                        p.finished.store(true, Ordering::SeqCst);
                    }
                }
                Cmd::SetVolume(id, volume) => {
                    if let Some(p) = playing.get_mut(&id) {
                        p.volume = volume;
                        p.player.set_volume(volume * master);
                    }
                }
                Cmd::Pause(id) => {
                    if let Some(p) = playing.get(&id) {
                        p.player.pause();
                    }
                }
                Cmd::Resume(id) => {
                    if let Some(p) = playing.get(&id) {
                        p.player.play();
                    }
                }
                Cmd::Master(volume) => {
                    master = volume;
                    for p in playing.values() {
                        p.player.set_volume(p.volume * master);
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // 鳴り終わった音を片づける
        playing.retain(|_, p| {
            let done = p.player.empty() && !p.player.is_paused();
            if done {
                p.finished.store(true, Ordering::SeqCst);
            }
            !done
        });
    }
}

/// 補助スレッドを（まだなければ）起こす。出力機器が開けなければエラー値
fn ensure_started() -> Result<(), Value> {
    if AUDIO.with(|a| a.borrow().is_some()) {
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || audio_thread(&rx, &ready_tx));
    match ready_rx.recv() {
        Ok(Ok(())) => {
            AUDIO.with(|a| {
                *a.borrow_mut() = Some(Audio {
                    tx,
                    sounds: HashMap::new(),
                    next_id: 1,
                });
            });
            Ok(())
        }
        Ok(Err(message)) => Err(error_value(
            "audio",
            format!("音の出力機器を開けません: {message}"),
        )),
        Err(_) => Err(error_value("audio", "音の出力機器を開けません")),
    }
}

fn send(cmd: Cmd) {
    AUDIO.with(|a| {
        if let Some(audio) = a.borrow().as_ref() {
            let _ = audio.tx.send(cmd);
        }
    });
}

// ---- 関数 ----

struct PlayOptions {
    volume: f32,
    speed: f32,
    looped: bool,
}

fn play_options(args: &[Value]) -> Result<PlayOptions, String> {
    let mut options = PlayOptions {
        volume: 1.0,
        speed: 1.0,
        looped: false,
    };
    let Some(value) = args.get(1) else {
        return Ok(options);
    };
    let Value::Map(map) = value else {
        return Err(type_error("audio.play", 1, "マップ", value));
    };
    for (key, value) in map.borrow().iter() {
        match (&**key, value) {
            ("volume", Value::Num(n)) if n.is_finite() && *n >= 0.0 => options.volume = *n as f32,
            ("speed", Value::Num(n)) if (0.1..=4.0).contains(n) => options.speed = *n as f32,
            ("loop", Value::Bool(b)) => options.looped = *b,
            ("volume", _) => {
                return Err("audio.play のオプション `volume` は 0 以上の数です".into());
            }
            ("speed", _) => return Err("audio.play のオプション `speed` は 0.1〜4 の数です".into()),
            ("loop", _) => return Err("audio.play のオプション `loop` は真偽値です".into()),
            _ => return Err(format!("audio.play のオプション `{key}` は知りません")),
        }
    }
    Ok(options)
}

fn play(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "audio.play の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let path = str_arg("audio.play", args, 0)?;
    let options = play_options(args)?;

    // ファイルを開いて、形式を確かめる（ここまでは出力機器に関係なく失敗を返せる）
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(error_value("not_found", format!("`{path}` がありません")));
        }
        Err(e) => return Ok(error_value("io", format!("`{path}` を開けません: {e}"))),
    };
    let source: BoxSource = if options.looped {
        match Decoder::new_looped(io::BufReader::new(file)) {
            Ok(d) => Box::new(d),
            Err(e) => return Ok(parse_error(path, &e)),
        }
    } else {
        match Decoder::try_from(file) {
            Ok(d) => Box::new(d),
            Err(e) => return Ok(parse_error(path, &e)),
        }
    };

    if let Err(e) = ensure_started() {
        return Ok(e);
    }
    let finished = Arc::new(AtomicBool::new(false));
    let id = AUDIO.with(|a| {
        let mut a = a.borrow_mut();
        let audio = a.as_mut().expect("ensure_started が作った");
        audio.sounds.retain(|_, f| !f.load(Ordering::SeqCst));
        let id = audio.next_id;
        audio.next_id += 1;
        audio.sounds.insert(id, Arc::clone(&finished));
        id
    });
    send(Cmd::Play {
        id,
        source,
        volume: options.volume,
        speed: options.speed,
        finished,
    });
    Ok(Value::Num(id as f64))
}

fn parse_error(path: &str, e: &dyn std::fmt::Display) -> Value {
    error_value("parse", format!("`{path}` を音として読めません: {e}"))
}

fn id_arg(name: &str, args: &[Value], i: usize) -> Result<u64, String> {
    let n = int_arg(name, args, i)?;
    // 知らない番号（負の数を含む）は、何も起きないだけ
    Ok(u64::try_from(n).unwrap_or(0))
}

fn volume_arg(name: &str, args: &[Value], i: usize, max: f64) -> Result<f32, String> {
    let n = num_arg(name, args, i)?;
    if n.is_finite() && (0.0..=max).contains(&n) {
        Ok(n as f32)
    } else {
        Err(format!("{name} の音量は 0〜{max} の数です（{n}）"))
    }
}

/// `stop[id]` などの、番号を 1 つ取って指示を送る関数
fn simple(name: &str, args: &[Value], make: fn(u64) -> Cmd) -> Result<Value, String> {
    expect_args(name, args, 1)?;
    let id = id_arg(name, args, 0)?;
    send(make(id));
    // `stop` はすぐに「止まった」と読めるようにする
    if name == "audio.stop" {
        AUDIO.with(|a| {
            if let Some(f) = a.borrow().as_ref().and_then(|a| a.sounds.get(&id)) {
                f.store(true, Ordering::SeqCst);
            }
        });
    }
    Ok(Value::Nil)
}

fn stop_all(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("audio.stop_all", args, 0)?;
    AUDIO.with(|a| {
        if let Some(audio) = a.borrow().as_ref() {
            for f in audio.sounds.values() {
                f.store(true, Ordering::SeqCst);
            }
        }
    });
    send(Cmd::StopAll);
    Ok(Value::Nil)
}

fn set_volume(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("audio.set_volume", args, 2)?;
    let id = id_arg("audio.set_volume", args, 0)?;
    let volume = {
        let n = num_arg("audio.set_volume", args, 1)?;
        if n.is_finite() && n >= 0.0 {
            n as f32
        } else {
            return Err(format!("audio.set_volume の音量は 0 以上の数です（{n}）"));
        }
    };
    send(Cmd::SetVolume(id, volume));
    Ok(Value::Nil)
}

fn set_master_volume(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("audio.set_master_volume", args, 1)?;
    let volume = volume_arg("audio.set_master_volume", args, 0, 2.0)?;
    send(Cmd::Master(volume));
    Ok(Value::Nil)
}

fn is_playing(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("audio.is_playing", args, 1)?;
    let id = id_arg("audio.is_playing", args, 0)?;
    Ok(Value::Bool(AUDIO.with(|a| {
        a.borrow()
            .as_ref()
            .and_then(|a| a.sounds.get(&id))
            .is_some_and(|finished| !finished.load(Ordering::SeqCst))
    })))
}

#[cfg(test)]
mod tests;
