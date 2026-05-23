use macroquad::prelude::*;
use macroquad::audio;
use super::{trim, Interpreter};

// ---- グラフィクスデータ型 ----

pub struct RectData {
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
    pub rgb: [u8; 3],
}

pub struct CircleData {
    pub x: f32, pub y: f32, pub radius: f32,
    pub rgb: [u8; 3],
}

pub struct TextData {
    pub x: f32, pub y: f32,
    pub content: String,
    pub font_size: u16,
    pub rgb: [u8; 3],
    pub font_id: String,
}

pub struct SpriteEntry {
    pub texture: Texture2D,
    pub flip_x: bool,
}

pub struct SpriteDrawCmd {
    pub texture: Texture2D,
    pub x: f32, pub y: f32,
    pub flip_x: bool,
}

pub struct RectDrawCmd {
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
    pub rgb: [u8; 3],
}

// ---- ルーティング ----

pub fn is_graphics_builtin(name: &str) -> bool {
    matches!(
        name,
        "window" | "rect_create" | "rect_set" | "rect_draw" | "key_check" | "bg"
        | "img_load" | "img_draw" | "img_flip"
        | "font_load" | "circle_create" | "circle_set"
        | "text_create" | "text_set" | "text_set_str"
        | "sound_load" | "sound_play" | "sound_vol"
        | "mouse_pos" | "mouse_click" | "camera_set"
    )
}

pub async fn call_graphics_builtin(interp: &mut Interpreter, name: &str, args: Vec<String>) {
    match name {
        "window" => builtin_window(interp, &args),
        "rect_create" => builtin_rect_create(interp, &args),
        "rect_set" => builtin_rect_set(interp, &args),
        "rect_draw" => builtin_rect_draw(interp, &args),
        "key_check" => builtin_key_check(interp, &args),
        "bg" => builtin_bg(interp, &args),
        "img_load" => builtin_img_load(interp, &args).await,
        "img_draw" => builtin_img_draw(interp, &args),
        "img_flip" => builtin_img_flip(interp, &args),
        "font_load" => builtin_font_load(interp, &args).await,
        "circle_create" => builtin_circle_create(interp, &args),
        "circle_set" => builtin_circle_set(interp, &args),
        "text_create" => builtin_text_create(interp, &args),
        "text_set" => builtin_text_set(interp, &args),
        "text_set_str" => builtin_text_set_str(interp, &args),
        "sound_load" => builtin_sound_load(interp, &args).await,
        "sound_play" => builtin_sound_play(interp, &args),
        "sound_vol" => builtin_sound_vol(interp, &args),
        "mouse_pos" => builtin_mouse_pos(interp, &args),
        "mouse_click" => builtin_mouse_click(interp, &args),
        "camera_set" => builtin_camera_set(interp, &args),
        _ => {}
    }
}

// ---- キーコード変換 ----

fn str_to_keycode(name: &str) -> Option<KeyCode> {
    match name {
        "Left" => Some(KeyCode::Left),
        "Right" => Some(KeyCode::Right),
        "Up" => Some(KeyCode::Up),
        "Down" => Some(KeyCode::Down),
        "Space" => Some(KeyCode::Space),
        "Enter" => Some(KeyCode::Enter),
        "Z" => Some(KeyCode::Z),
        "X" => Some(KeyCode::X),
        "A" => Some(KeyCode::A),
        "D" => Some(KeyCode::D),
        "W" => Some(KeyCode::W),
        "S" => Some(KeyCode::S),
        _ => None,
    }
}

fn mk_color(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

fn parse_u8(s: &str) -> u8 {
    s.parse::<f64>().unwrap_or(0.0) as u8
}

fn parse_f32(s: &str) -> f32 {
    s.parse::<f32>().unwrap_or(0.0)
}

// ---- 各組み込み関数 ----

// window[width, height, title?]
pub fn builtin_window(interp: &mut Interpreter, args: &[String]) {
    let args = interp.eval_args(args);
    if args.len() < 2 {
        eprintln!("Error: window requires width and height.");
        return;
    }
    interp.window_open = true;
    // macroquadのウィンドウはすでに開いているので、サイズ変更をリクエスト
    let w = parse_f32(&args[0]);
    let h = parse_f32(&args[1]);
    request_new_screen_size(w, h);
}

// rect_create[id, x, y, w, h, r, g, b]
pub fn builtin_rect_create(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 8 {
        eprintln!("Error: rect_create requires id x y w h r g b.");
        return;
    }
    let id = args[0].clone();
    let n: Vec<String> = interp.eval_args(&args[1..]);
    let data = RectData {
        x: parse_f32(&n[0]), y: parse_f32(&n[1]),
        w: parse_f32(&n[2]), h: parse_f32(&n[3]),
        rgb: [parse_u8(&n[4]), parse_u8(&n[5]), parse_u8(&n[6])],
    };
    if let Some(&idx) = interp.shape_index.get(&id) {
        interp.shapes[idx].1 = data;
    } else {
        let idx = interp.shapes.len();
        interp.shape_index.insert(id.clone(), idx);
        interp.shapes.push((id, data));
    }
}

// rect_set[id, x, y]
pub fn builtin_rect_set(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: rect_set requires id x y.");
        return;
    }
    let id = args[0].clone();
    let n: Vec<String> = interp.eval_args(&args[1..]);
    if let Some(&idx) = interp.shape_index.get(&id) {
        interp.shapes[idx].1.x = parse_f32(&n[0]);
        interp.shapes[idx].1.y = parse_f32(&n[1]);
    } else {
        eprintln!("Error: rect '{}' not found.", id);
    }
}

// rect_draw[x, y, w, h, r, g, b]  — このフレームのみ
pub fn builtin_rect_draw(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 7 {
        eprintln!("Error: rect_draw requires x y w h r g b.");
        return;
    }
    let n: Vec<String> = interp.eval_args(args);
    interp.rect_queue.push(RectDrawCmd {
        x: parse_f32(&n[0]), y: parse_f32(&n[1]),
        w: parse_f32(&n[2]), h: parse_f32(&n[3]),
        rgb: [parse_u8(&n[4]), parse_u8(&n[5]), parse_u8(&n[6])],
    });
}

// key_check[KeyName, @var]
pub fn builtin_key_check(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: key_check requires key_name and var.");
        return;
    }
    let key_name = interp.resolve_var(trim(&args[0]));
    let var_name = trim(&args[1]).trim_start_matches('@').to_string();
    let pressed = str_to_keycode(&key_name)
        .map(|k| is_key_down(k))
        .unwrap_or(false);
    interp.set_var(&var_name, if pressed { "1" } else { "0" }.to_string());
}

// bg[r, g, b]
pub fn builtin_bg(interp: &mut Interpreter, args: &[String]) {
    let n = interp.eval_args(args);
    if n.len() < 3 { return; }
    interp.bg = [parse_u8(&n[0]), parse_u8(&n[1]), parse_u8(&n[2])];
}

// img_load[id, path]
pub async fn builtin_img_load(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: img_load requires id and path.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let path = trim(&args[1]);
    match load_texture(path).await {
        Ok(texture) => {
            interp.sprites.insert(id, SpriteEntry { texture, flip_x: false });
        }
        Err(e) => eprintln!("Error: Failed to load image '{}': {}", path, e),
    }
}

// img_draw[id, x, y]
pub fn builtin_img_draw(interp: &mut Interpreter, args: &[String]) {
    let n = interp.eval_args(args);
    if n.len() < 3 {
        eprintln!("Error: img_draw requires id, x, y.");
        return;
    }
    let id = &n[0];
    if let Some(entry) = interp.sprites.get(id) {
        interp.sprite_queue.push(SpriteDrawCmd {
            texture: entry.texture.clone(),
            x: parse_f32(&n[1]),
            y: parse_f32(&n[2]),
            flip_x: entry.flip_x,
        });
    } else {
        eprintln!("Error: Image '{}' not loaded.", id);
    }
}

// img_flip[id, 1/0]
pub fn builtin_img_flip(interp: &mut Interpreter, args: &[String]) {
    let n = interp.eval_args(args);
    if n.len() < 2 { return; }
    let id = &n[0];
    if let Some(entry) = interp.sprites.get_mut(id) {
        entry.flip_x = n[1] == "1";
    }
}

// font_load[id, path]
pub async fn builtin_font_load(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: font_load requires id and path.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let path = trim(&args[1]);
    match load_ttf_font(path).await {
        Ok(font) => { interp.fonts.insert(id, font); }
        Err(e) => eprintln!("Error: Failed to load font '{}': {}", path, e),
    }
}

// circle_create[id, x, y, radius, r, g, b]
pub fn builtin_circle_create(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 7 {
        eprintln!("Error: circle_create requires id x y radius r g b.");
        return;
    }
    let id = args[0].clone();
    let n: Vec<String> = interp.eval_args(&args[1..]);
    let data = CircleData {
        x: parse_f32(&n[0]), y: parse_f32(&n[1]),
        radius: parse_f32(&n[2]),
        rgb: [parse_u8(&n[3]), parse_u8(&n[4]), parse_u8(&n[5])],
    };
    if let Some(&idx) = interp.circle_index.get(&id) {
        interp.circles[idx].1 = data;
    } else {
        let idx = interp.circles.len();
        interp.circle_index.insert(id.clone(), idx);
        interp.circles.push((id, data));
    }
}

// circle_set[id, x, y]
pub fn builtin_circle_set(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: circle_set requires id x y.");
        return;
    }
    let id = args[0].clone();
    let n: Vec<String> = interp.eval_args(&args[1..]);
    if let Some(&idx) = interp.circle_index.get(&id) {
        interp.circles[idx].1.x = parse_f32(&n[0]);
        interp.circles[idx].1.y = parse_f32(&n[1]);
    } else {
        eprintln!("Error: circle '{}' not found.", id);
    }
}

// text_create[id, font_id, x, y, str, size, r, g, b]
pub fn builtin_text_create(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 9 {
        eprintln!("Error: text_create requires id font_id x y str size r g b.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let font_id = trim(&args[1]).to_string();
    let n: Vec<String> = interp.eval_args(&args[2..]);
    // n: x, y, str, size, r, g, b
    let data = TextData {
        x: parse_f32(&n[0]), y: parse_f32(&n[1]),
        content: n[2].clone(),
        font_size: n[3].parse::<u16>().unwrap_or(20),
        rgb: [parse_u8(&n[4]), parse_u8(&n[5]), parse_u8(&n[6])],
        font_id,
    };
    if let Some(&idx) = interp.text_index.get(&id) {
        interp.texts[idx].1 = data;
    } else {
        let idx = interp.texts.len();
        interp.text_index.insert(id.clone(), idx);
        interp.texts.push((id, data));
    }
}

// text_set[id, x, y]
pub fn builtin_text_set(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: text_set requires id x y.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let n: Vec<String> = interp.eval_args(&args[1..]);
    if let Some(&idx) = interp.text_index.get(&id) {
        interp.texts[idx].1.x = parse_f32(&n[0]);
        interp.texts[idx].1.y = parse_f32(&n[1]);
    } else {
        eprintln!("Error: text '{}' not found.", id);
    }
}

// text_set_str[id, str]
pub fn builtin_text_set_str(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: text_set_str requires id and str.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let s = interp.interpolate(trim(&args[1]));
    if let Some(&idx) = interp.text_index.get(&id) {
        interp.texts[idx].1.content = s;
    } else {
        eprintln!("Error: text '{}' not found.", id);
    }
}

// sound_load[id, path]
pub async fn builtin_sound_load(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: sound_load requires id and path.");
        return;
    }
    let id = trim(&args[0]).to_string();
    let path = trim(&args[1]);
    match audio::load_sound(path).await {
        Ok(sound) => { interp.sounds.insert(id, sound); }
        Err(e) => eprintln!("Error: Failed to load sound '{}': {}", path, e),
    }
}

// sound_play[id]
pub fn builtin_sound_play(interp: &mut Interpreter, args: &[String]) {
    if args.is_empty() { return; }
    let id = trim(&args[0]);
    if let Some(sound) = interp.sounds.get(id) {
        audio::play_sound(sound, audio::PlaySoundParams { looped: false, volume: 1.0 });
    } else {
        eprintln!("Error: Sound '{}' not loaded.", id);
    }
}

// sound_vol[id, volume]  0〜100
pub fn builtin_sound_vol(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 { return; }
    let id = trim(&args[0]);
    let vol = interp.resolve_var(trim(&args[1])).parse::<f32>().unwrap_or(100.0) / 100.0;
    if let Some(sound) = interp.sounds.get(id) {
        audio::set_sound_volume(sound, vol);
    }
}

// mouse_pos[@x, @y]
pub fn builtin_mouse_pos(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 { return; }
    let (mx, my) = mouse_position();
    let xv = trim(&args[0]).trim_start_matches('@').to_string();
    let yv = trim(&args[1]).trim_start_matches('@').to_string();
    interp.set_var(&xv, ((mx + interp.camera_x) as i64).to_string());
    interp.set_var(&yv, ((my + interp.camera_y) as i64).to_string());
}

// mouse_click[@var]
pub fn builtin_mouse_click(interp: &mut Interpreter, args: &[String]) {
    if args.is_empty() { return; }
    let v = trim(&args[0]).trim_start_matches('@').to_string();
    let clicked = interp.mouse_clicked;
    interp.set_var(&v, if clicked { "1" } else { "0" }.to_string());
}

// camera_set[x, y]
pub fn builtin_camera_set(interp: &mut Interpreter, args: &[String]) {
    let n = interp.eval_args(args);
    if n.len() < 2 { return; }
    interp.camera_x = parse_f32(&n[0]);
    interp.camera_y = parse_f32(&n[1]);
}

// ---- ゲームループ ----

pub async fn run_gameloop(interp: &mut Interpreter, body: &str) {
    if !interp.window_open {
        eprintln!("Error: call window[] before gameloop.");
        return;
    }

    let body = body.to_string();
    loop {
        // イベント処理
        if is_key_down(KeyCode::Escape) {
            break;
        }
        interp.mouse_clicked = is_mouse_button_pressed(MouseButton::Left);
        interp.sprite_queue.clear();
        interp.rect_queue.clear();

        // ゲームロジック実行
        interp.exec(&body).await;

        // 描画開始
        clear_background(mk_color(interp.bg[0], interp.bg[1], interp.bg[2]));

        // カメラ適用（y下向きのスクリーン座標系 = SFML互換）
        // macroquad は内部で invert_y=-1 を掛けるので zoom.y を正にすると y が下向きになる
        let cam = Camera2D {
            target: vec2(
                interp.camera_x + screen_width() / 2.0,
                interp.camera_y + screen_height() / 2.0,
            ),
            zoom: vec2(2.0 / screen_width(), 2.0 / screen_height()),
            ..Default::default()
        };
        set_camera(&cam);

        for (_, rect) in &interp.shapes {
            draw_rectangle(rect.x, rect.y, rect.w, rect.h, mk_color(rect.rgb[0], rect.rgb[1], rect.rgb[2]));
        }
        for (_, circle) in &interp.circles {
            draw_circle(circle.x, circle.y, circle.radius, mk_color(circle.rgb[0], circle.rgb[1], circle.rgb[2]));
        }
        // フレームのみ矩形
        let rect_cmds: Vec<RectDrawCmd> = std::mem::take(&mut interp.rect_queue);
        for cmd in &rect_cmds {
            draw_rectangle(cmd.x, cmd.y, cmd.w, cmd.h, mk_color(cmd.rgb[0], cmd.rgb[1], cmd.rgb[2]));
        }
        // スプライト
        let sprite_cmds: Vec<SpriteDrawCmd> = std::mem::take(&mut interp.sprite_queue);
        for cmd in sprite_cmds {
            let params = DrawTextureParams {
                flip_x: cmd.flip_x,
                ..Default::default()
            };
            draw_texture_ex(&cmd.texture, cmd.x, cmd.y, WHITE, params);
        }
        // テキスト
        let fonts = &interp.fonts;
        for (_, text) in &interp.texts {
            let color = mk_color(text.rgb[0], text.rgb[1], text.rgb[2]);
            if let Some(font) = fonts.get(&text.font_id) {
                draw_text_ex(
                    &text.content,
                    text.x,
                    text.y,
                    TextParams {
                        font: Some(font),
                        font_size: text.font_size,
                        color,
                        ..Default::default()
                    },
                );
            } else {
                draw_text(&text.content, text.x, text.y, text.font_size as f32, color);
            }
        }

        set_default_camera();
        next_frame().await;
    }
}
