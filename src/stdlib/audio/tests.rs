//! 引数・ファイルの検査と、音の番号の管理のテスト（出力機器がなくても通る）

use super::*;
use crate::syntax::{lexer, parser};

fn eval(interp: &mut Interpreter<'_>, src: &str) -> Result<Value, String> {
    let tokens = lexer::tokenize(src).map_err(|e| e.message)?;
    let program = parser::parse(&tokens).map_err(|e| e.message)?;
    interp
        .run(&program, true)
        .map(|v| v.unwrap_or(Value::Nil))
        .map_err(|e| e.message)
}

fn with_audio(test: impl FnOnce(&mut Interpreter<'_>)) {
    let mut out = Vec::new();
    let mut interp = Interpreter::new(&mut out, ".".into());
    crate::stdlib::register(&mut interp);
    register(&mut interp);
    eval(&mut interp, "@a := import[\"std:audio\"]").unwrap();
    test(&mut interp);
}

/// 8000 Hz・16 ビット・モノラルの WAV（`ms` ミリ秒の 440 Hz の音）
fn wav(ms: u32) -> Vec<u8> {
    let samples = 8 * ms;
    let mut data = Vec::new();
    for i in 0..samples {
        let t = f64::from(i) / 8000.0;
        let v = ((t * 440.0 * std::f64::consts::TAU).sin() * 3000.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // モノラル
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn slash(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[test]
fn argument_errors() {
    with_audio(|interp| {
        for (src, expected) in [
            ("@a.play[]", "引数は 1 個か 2 個"),
            ("@a.play[1]", "文字列"),
            ("@a.play[\"x.wav\", 5]", "マップ"),
            ("@a.play[\"x.wav\", {\"volume\": -1}]", "volume"),
            ("@a.play[\"x.wav\", {\"speed\": 9}]", "speed"),
            ("@a.play[\"x.wav\", {\"loop\": 1}]", "loop"),
            ("@a.play[\"x.wav\", {\"nope\": 1}]", "知りません"),
            ("@a.stop[\"x\"]", "数値"),
            ("@a.stop[1.5]", "整数"),
            ("@a.set_volume[1, -1]", "0 以上"),
            ("@a.set_master_volume[3]", "0〜2"),
            ("@a.is_playing[]", "引数は 1 個"),
            ("@a.stop_all[1]", "引数は 0 個"),
        ] {
            let err = eval(interp, src).unwrap_err();
            assert!(err.contains(expected), "{src}: {err}");
        }
    });
}

#[test]
fn file_errors_are_values() {
    let dir = std::env::temp_dir().join("dolphin-audio-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("not-audio.wav");
    std::fs::write(&text, "this is not a sound").unwrap();
    with_audio(|interp| {
        let kind = |interp: &mut Interpreter<'_>, path: &str| {
            eval(interp, &format!("err_kind[@a.play[\"{path}\"]]"))
                .unwrap()
                .to_string()
        };
        assert_eq!(kind(interp, &slash(&dir.join("missing.wav"))), "not_found");
        assert_eq!(kind(interp, &slash(&text)), "parse");
        // 知らない番号は、何も起きない
        assert_eq!(
            eval(interp, "@a.is_playing[12345]").unwrap().to_string(),
            "false"
        );
        eval(interp, "@a.stop[12345]").unwrap();
        eval(interp, "@a.pause[-1]").unwrap();
        eval(interp, "@a.set_volume[99, 0.5]").unwrap();
        eval(interp, "@a.stop_all[]").unwrap();
    });
}

#[test]
fn play_and_stop_if_there_is_an_output_device() {
    let dir = std::env::temp_dir().join("dolphin-audio-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("beep.wav");
    std::fs::write(&path, wav(3000)).unwrap();
    let path = slash(&path);
    with_audio(|interp| {
        eval(
            interp,
            &format!("@id := @a.play[\"{path}\", {{\"volume\": 0, \"speed\": 2}}]"),
        )
        .unwrap();
        let id = eval(interp, "@id").unwrap();
        if let Value::Error(e) = &id {
            // 出力機器のない環境（CI など）
            assert_eq!(&*e.kind, "audio");
            return;
        }
        assert_eq!(
            eval(interp, "@a.is_playing[@id]").unwrap().to_string(),
            "true"
        );
        eval(interp, "@a.pause[@id]").unwrap();
        assert_eq!(
            eval(interp, "@a.is_playing[@id]").unwrap().to_string(),
            "true"
        );
        eval(interp, "@a.resume[@id]").unwrap();
        eval(interp, "@a.set_volume[@id, 0]").unwrap();
        eval(interp, "@a.set_master_volume[0]").unwrap();
        eval(interp, "@a.stop[@id]").unwrap();
        assert_eq!(
            eval(interp, "@a.is_playing[@id]").unwrap().to_string(),
            "false"
        );

        // 2 つ目は別の番号。stop_all で全部止まる
        eval(
            interp,
            &format!("@b := @a.play[\"{path}\", {{\"loop\": true, \"volume\": 0}}]"),
        )
        .unwrap();
        let second = eval(interp, "@b").unwrap();
        assert_ne!(second.to_string(), id.to_string());
        eval(interp, "@a.stop_all[]").unwrap();
        assert_eq!(
            eval(interp, "@a.is_playing[@b]").unwrap().to_string(),
            "false"
        );
    });
}

#[test]
fn options_are_parsed() {
    with_audio(|interp| {
        let value = eval(interp, "{\"volume\": 0.5, \"speed\": 1.5, \"loop\": true}").unwrap();
        let options = play_options(&[Value::Nil, value]).unwrap();
        assert_eq!(options.volume, 0.5);
        assert_eq!(options.speed, 1.5);
        assert!(options.looped);
        let defaults = play_options(&[Value::Nil]).unwrap();
        assert_eq!(
            (defaults.volume, defaults.speed, defaults.looped),
            (1.0, 1.0, false)
        );
    });
}
