//! `std:image`: info, resize, crop, convert（design.md 5.36）
//!
//! 画像のバイト列を受け取って、バイト列を返す。GUI は使わない

use std::io::Cursor;

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, Frame, ImageFormat};
#[cfg(test)]
use image::{Rgba, RgbaImage};

use super::{bytes_arg, error_value, int_arg, new_map, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

/// 縦横それぞれの上限（画素）
const MAX_SIDE: u32 = 16384;
/// デコードする画素数の上限
const MAX_PIXELS: u64 = 100_000_000;
const DEFAULT_QUALITY: u8 = 85;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:image",
        &[
            ("image.info", info),
            ("image.resize", resize),
            ("image.crop", crop),
            ("image.convert", convert),
        ],
    );
}

// ---- 引数 ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Exact,
    Inside,
    Cover,
}

struct Options {
    format: Option<ImageFormat>,
    quality: u8,
    fit: Fit,
    filter: FilterType,
}

fn format_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Gif => "gif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::WebP => "webp",
        _ => "unknown",
    }
}

fn parse_format(name: &str, value: &str) -> Result<ImageFormat, String> {
    match value {
        "png" => Ok(ImageFormat::Png),
        "jpeg" => Ok(ImageFormat::Jpeg),
        "gif" => Ok(ImageFormat::Gif),
        _ => Err(format!(
            "{name} の `format` は \"png\" \"jpeg\" \"gif\" のどれかです（\"{value}\" が渡されました）"
        )),
    }
}

/// `args[i]` のオプションのマップ（なければ既定）を読む。`allowed` にないキーは実行時エラー
fn options(name: &str, args: &[Value], i: usize, allowed: &[&str]) -> Result<Options, String> {
    let mut opts = Options {
        format: None,
        quality: DEFAULT_QUALITY,
        fit: Fit::Exact,
        filter: FilterType::Triangle,
    };
    let Some(arg) = args.get(i) else {
        return Ok(opts);
    };
    let Value::Map(map) = arg else {
        return Err(type_error(name, i, "マップ", arg));
    };
    for (key, value) in map.borrow().iter() {
        let key: &str = key;
        if !allowed.contains(&key) {
            return Err(format!("{name} のオプション `{key}` は知りません"));
        }
        let bad_type = |expected: &str| {
            format!(
                "{name} のオプション `{key}` は{expected}です（{}が渡されました）",
                value.type_name()
            )
        };
        let text = |expected: &str| match value {
            Value::Str(s) => Ok(s.to_string()),
            _ => Err(bad_type(expected)),
        };
        match key {
            "format" => opts.format = Some(parse_format(name, &text("文字列")?)?),
            "quality" => {
                let Value::Num(q) = value else {
                    return Err(bad_type("数値"));
                };
                if q.fract() != 0.0 || !(1.0..=100.0).contains(q) {
                    return Err(format!(
                        "{name} の `quality` は 1〜100 の整数です（{q} が渡されました）"
                    ));
                }
                opts.quality = *q as u8;
            }
            "fit" => {
                opts.fit = match text("文字列")?.as_str() {
                    "exact" => Fit::Exact,
                    "inside" => Fit::Inside,
                    "cover" => Fit::Cover,
                    other => {
                        return Err(format!(
                            "{name} の `fit` は \"exact\" \"inside\" \"cover\" のどれかです（\"{other}\" が渡されました）"
                        ));
                    }
                }
            }
            "filter" => {
                opts.filter = match text("文字列")?.as_str() {
                    "bilinear" => FilterType::Triangle,
                    "nearest" => FilterType::Nearest,
                    "lanczos" => FilterType::Lanczos3,
                    other => {
                        return Err(format!(
                            "{name} の `filter` は \"bilinear\" \"nearest\" \"lanczos\" のどれかです（\"{other}\" が渡されました）"
                        ));
                    }
                }
            }
            _ => unreachable!("allowed で絞った"),
        }
    }
    Ok(opts)
}

/// 1 以上の整数の引数
fn size_arg(name: &str, args: &[Value], i: usize) -> Result<u32, String> {
    let n = int_arg(name, args, i)?;
    u32::try_from(n)
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| format!("{name} の {} 番目の引数は 1 以上の整数です（{n}）", i + 1))
}

/// 0 以上の整数の引数
fn offset_arg(name: &str, args: &[Value], i: usize) -> Result<u32, String> {
    let n = int_arg(name, args, i)?;
    u32::try_from(n)
        .map_err(|_| format!("{name} の {} 番目の引数は 0 以上の整数です（{n}）", i + 1))
}

fn arg_count(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else if min == max {
        Err(format!(
            "{name} の引数は {min} 個です（{} 個渡されました）",
            args.len()
        ))
    } else {
        Err(format!(
            "{name} の引数は {min} 個以上 {max} 個以下です（{} 個渡されました）",
            args.len()
        ))
    }
}

// ---- 読み込み ----

/// 失敗は（種類, メッセージ）。`Err` をそのままエラー値にする
type Failure = (&'static str, String);

fn failure(f: Failure) -> Value {
    error_value(f.0, f.1)
}

fn parse_error(e: impl std::fmt::Display) -> Failure {
    ("parse", format!("画像を読めません: {e}"))
}

fn too_large(width: u32, height: u32) -> Failure {
    (
        "too_large",
        format!(
            "画像が大きすぎます（{width} × {height}。縦横 {MAX_SIDE} 画素、全体 {} 億画素まで）",
            MAX_PIXELS / 100_000_000
        ),
    )
}

fn check_size(width: u32, height: u32) -> Result<(), Failure> {
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        Err(too_large(width, height))
    } else {
        Ok(())
    }
}

/// 先頭のバイトから形式を決め、ヘッダーだけ読んで大きさを調べる
fn probe(data: &[u8]) -> Result<(ImageFormat, u32, u32), Failure> {
    let format = image::guess_format(data).map_err(parse_error)?;
    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::WebP
    ) {
        return Err(("parse", "対応していない形式です".to_string()));
    }
    let (width, height) = image::ImageReader::with_format(Cursor::new(data), format)
        .into_dimensions()
        .map_err(parse_error)?;
    Ok((format, width, height))
}

fn decode(data: &[u8]) -> Result<(ImageFormat, DynamicImage), Failure> {
    let (format, width, height) = probe(data)?;
    check_size(width, height)?;
    let img = image::load_from_memory_with_format(data, format).map_err(parse_error)?;
    Ok((format, img))
}

/// 動くGIF の全フレーム（各フレームは全画面に合成済み）。動かない画像は None
fn gif_frames(data: &[u8]) -> Result<Option<Vec<Frame>>, Failure> {
    let decoder = GifDecoder::new(Cursor::new(data)).map_err(parse_error)?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(parse_error)?;
    Ok((frames.len() > 1).then_some(frames))
}

// ---- 書き出し ----

/// 透明を白の上に重ねた RGB（JPEG は透明を持てない）
fn flatten(img: &DynamicImage) -> image::RgbImage {
    let rgba = img.to_rgba8();
    let mut out = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = u32::from(p[3]);
        let mix = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])]));
    }
    out
}

fn encode(img: &DynamicImage, format: ImageFormat, quality: u8) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    let result = match format {
        ImageFormat::Jpeg => {
            JpegEncoder::new_with_quality(&mut out, quality).encode_image(&flatten(img))
        }
        _ => img.write_to(&mut Cursor::new(&mut out), format),
    };
    result.map_err(|e| ("parse", format!("画像を書き出せません: {e}")))?;
    Ok(out)
}

fn encode_gif(frames: Vec<Frame>) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        let fail = |e: image::ImageError| ("parse", format!("画像を書き出せません: {e}"));
        encoder.set_repeat(Repeat::Infinite).map_err(fail)?;
        encoder.encode_frames(frames).map_err(fail)?;
    }
    Ok(out)
}

/// 出力の形式。指定がなければ入力と同じ（`bmp` `webp` は `png`）
fn output_format(requested: Option<ImageFormat>, input: ImageFormat) -> ImageFormat {
    requested.unwrap_or(match input {
        ImageFormat::Jpeg | ImageFormat::Gif => input,
        _ => ImageFormat::Png,
    })
}

// ---- 加工 ----

/// `fit` に従った出力の大きさ。`Inside` は縦横比を保ち、小さい画像は拡大しない
fn target_size(fit: Fit, (iw, ih): (u32, u32), (w, h): (u32, u32)) -> (u32, u32) {
    match fit {
        Fit::Exact | Fit::Cover => (w, h),
        Fit::Inside => {
            if iw <= w && ih <= h {
                return (iw, ih);
            }
            let scale = (f64::from(w) / f64::from(iw)).min(f64::from(h) / f64::from(ih));
            let round = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
            (round(iw), round(ih))
        }
    }
}

fn resize_image(
    img: &DynamicImage,
    fit: Fit,
    filter: FilterType,
    size: (u32, u32),
) -> DynamicImage {
    let (w, h) = size;
    if (img.width(), img.height()) == (w, h) {
        return img.clone();
    }
    match fit {
        Fit::Cover => img.resize_to_fill(w, h, filter),
        _ => img.resize_exact(w, h, filter),
    }
}

// ---- 関数 ----

fn info(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    arg_count("image.info", args, 1, 1)?;
    let data = bytes_arg("image.info", args, 0)?;
    Ok(match probe(data) {
        Ok((format, width, height)) => {
            let mut map = Map::default();
            map.insert("format".into(), Value::Str(format_name(format).into()));
            map.insert("width".into(), Value::Num(f64::from(width)));
            map.insert("height".into(), Value::Num(f64::from(height)));
            new_map(map)
        }
        Err(e) => failure(e),
    })
}

fn resize(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "image.resize";
    arg_count(NAME, args, 3, 4)?;
    let data = bytes_arg(NAME, args, 0)?;
    let (w, h) = (size_arg(NAME, args, 1)?, size_arg(NAME, args, 2)?);
    let opts = options(NAME, args, 3, &["format", "quality", "fit", "filter"])?;
    if w > MAX_SIDE || h > MAX_SIDE {
        return Ok(failure(too_large(w, h)));
    }
    Ok(match resize_bytes(data, w, h, &opts) {
        Ok(bytes) => Value::bytes(bytes),
        Err(e) => failure(e),
    })
}

fn resize_bytes(data: &[u8], w: u32, h: u32, opts: &Options) -> Result<Vec<u8>, Failure> {
    let (input, iw, ih) = probe(data)?;
    check_size(iw, ih)?;
    let format = output_format(opts.format, input);
    let size = target_size(opts.fit, (iw, ih), (w, h));
    // 動く GIF は、GIF で出すときだけ全フレームを加工する
    if input == ImageFormat::Gif
        && format == ImageFormat::Gif
        && let Some(frames) = gif_frames(data)?
    {
        let frames = frames
            .into_iter()
            .map(|frame| {
                let delay = frame.delay();
                let img = DynamicImage::ImageRgba8(frame.into_buffer());
                let resized = resize_image(&img, opts.fit, opts.filter, size);
                Frame::from_parts(resized.to_rgba8(), 0, 0, delay)
            })
            .collect();
        return encode_gif(frames);
    }
    let (_, img) = decode(data)?;
    encode(
        &resize_image(&img, opts.fit, opts.filter, size),
        format,
        opts.quality,
    )
}

fn crop(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "image.crop";
    arg_count(NAME, args, 5, 6)?;
    let data = bytes_arg(NAME, args, 0)?;
    let (x, y) = (offset_arg(NAME, args, 1)?, offset_arg(NAME, args, 2)?);
    let (w, h) = (size_arg(NAME, args, 3)?, size_arg(NAME, args, 4)?);
    let opts = options(NAME, args, 5, &["format", "quality"])?;
    Ok(match crop_bytes(data, (x, y, w, h), &opts) {
        Ok(bytes) => Value::bytes(bytes),
        Err(e) => failure(e),
    })
}

fn crop_bytes(
    data: &[u8],
    (x, y, w, h): (u32, u32, u32, u32),
    opts: &Options,
) -> Result<Vec<u8>, Failure> {
    let (input, iw, ih) = probe(data)?;
    check_size(iw, ih)?;
    if u64::from(x) + u64::from(w) > u64::from(iw) || u64::from(y) + u64::from(h) > u64::from(ih) {
        return Err((
            "range",
            format!("切り出す範囲（{x}, {y}, {w} × {h}）が画像（{iw} × {ih}）からはみ出しています"),
        ));
    }
    let format = output_format(opts.format, input);
    if input == ImageFormat::Gif
        && format == ImageFormat::Gif
        && let Some(frames) = gif_frames(data)?
    {
        let frames = frames
            .into_iter()
            .map(|frame| {
                let delay = frame.delay();
                let img = DynamicImage::ImageRgba8(frame.into_buffer());
                Frame::from_parts(img.crop_imm(x, y, w, h).to_rgba8(), 0, 0, delay)
            })
            .collect();
        return encode_gif(frames);
    }
    let (_, img) = decode(data)?;
    encode(&img.crop_imm(x, y, w, h), format, opts.quality)
}

fn convert(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    const NAME: &str = "image.convert";
    arg_count(NAME, args, 2, 3)?;
    let data = bytes_arg(NAME, args, 0)?;
    let format = parse_format(NAME, str_arg(NAME, args, 1)?)?;
    let opts = options(NAME, args, 2, &["quality"])?;
    Ok(match convert_bytes(data, format, opts.quality) {
        Ok(bytes) => Value::bytes(bytes),
        Err(e) => failure(e),
    })
}

fn convert_bytes(data: &[u8], format: ImageFormat, quality: u8) -> Result<Vec<u8>, Failure> {
    let (input, iw, ih) = probe(data)?;
    check_size(iw, ih)?;
    if input == ImageFormat::Gif && format == ImageFormat::Gif {
        // 同じ形式への変換は、動きも含めてそのまま
        if let Some(frames) = gif_frames(data)? {
            return encode_gif(frames);
        }
    }
    let (_, img) = decode(data)?;
    encode(&img, format, quality)
}

/// テストで使う、ピクセルが分かる小さな画像
#[cfg(test)]
fn test_image(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
    let mut img = RgbaImage::new(width, height);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = Rgba([
            (x * 255 / width.max(1)) as u8,
            (y * 255 / height.max(1)) as u8,
            128,
            255,
        ]);
    }
    encode(&DynamicImage::ImageRgba8(img), format, 90).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(data: &[u8]) -> (ImageFormat, u32, u32) {
        probe(data).unwrap()
    }

    fn opts(fit: Fit, format: Option<ImageFormat>) -> Options {
        Options {
            format,
            quality: 85,
            fit,
            filter: FilterType::Triangle,
        }
    }

    #[test]
    fn info_reads_header() {
        for (format, name) in [
            (ImageFormat::Png, "png"),
            (ImageFormat::Jpeg, "jpeg"),
            (ImageFormat::Gif, "gif"),
        ] {
            let data = test_image(40, 30, format);
            let (f, w, h) = dims(&data);
            assert_eq!((format_name(f), w, h), (name, 40, 30));
        }
        assert_eq!(probe(b"not an image").unwrap_err().0, "parse");
    }

    #[test]
    fn fit_modes() {
        let data = test_image(200, 100, ImageFormat::Png);
        let out = resize_bytes(&data, 50, 50, &opts(Fit::Exact, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (50, 50));
        let out = resize_bytes(&data, 50, 50, &opts(Fit::Inside, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (50, 25));
        let out = resize_bytes(&data, 50, 50, &opts(Fit::Cover, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (50, 50));
        // 小さい画像は拡大しない
        let out = resize_bytes(&data, 1000, 1000, &opts(Fit::Inside, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (200, 100));
    }

    #[test]
    fn format_conversion_and_default() {
        let png = test_image(20, 20, ImageFormat::Png);
        let jpeg = resize_bytes(&png, 10, 10, &opts(Fit::Exact, Some(ImageFormat::Jpeg))).unwrap();
        assert_eq!(dims(&jpeg).0, ImageFormat::Jpeg);
        // 形式を指定しなければ入力と同じ
        let same = resize_bytes(&jpeg, 5, 5, &opts(Fit::Exact, None)).unwrap();
        assert_eq!(dims(&same).0, ImageFormat::Jpeg);
        let gif = convert_bytes(&png, ImageFormat::Gif, 85).unwrap();
        assert_eq!(dims(&gif).0, ImageFormat::Gif);
    }

    #[test]
    fn jpeg_flattens_transparency_onto_white() {
        let mut img = RgbaImage::new(8, 8);
        for p in img.pixels_mut() {
            *p = Rgba([0, 0, 0, 0]);
        }
        let png = encode(&DynamicImage::ImageRgba8(img), ImageFormat::Png, 85).unwrap();
        let jpeg = convert_bytes(&png, ImageFormat::Jpeg, 95).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        let p = back.get_pixel(4, 4);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "{p:?}");
    }

    #[test]
    fn crop_checks_bounds() {
        let data = test_image(40, 30, ImageFormat::Png);
        let out = crop_bytes(&data, (10, 10, 20, 10), &opts(Fit::Exact, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (20, 10));
        let err = crop_bytes(&data, (30, 0, 20, 10), &opts(Fit::Exact, None)).unwrap_err();
        assert_eq!(err.0, "range");
    }

    #[test]
    fn animated_gif_keeps_frames_and_delays() {
        let frame = |shade: u8| {
            Frame::from_parts(
                RgbaImage::from_pixel(20, 10, Rgba([shade, 0, 0, 255])),
                0,
                0,
                image::Delay::from_numer_denom_ms(100, 1),
            )
        };
        let gif = encode_gif(vec![frame(50), frame(200), frame(120)]).unwrap();
        let out = resize_bytes(&gif, 10, 10, &opts(Fit::Inside, None)).unwrap();
        assert_eq!((dims(&out).1, dims(&out).2), (10, 5));
        let frames = gif_frames(&out).unwrap().expect("動く GIF のまま");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].delay().numer_denom_ms(), (100, 1));
        // GIF 以外で出すと、最初のフレームだけ
        let png = resize_bytes(&gif, 10, 10, &opts(Fit::Inside, Some(ImageFormat::Png))).unwrap();
        assert_eq!(dims(&png).0, ImageFormat::Png);
    }

    #[test]
    fn size_limits() {
        // 縦横の上限を超える大きさのヘッダーだけの PNG
        let mut png = test_image(1, 1, ImageFormat::Png);
        // IHDR の幅（先頭から 16 バイト目）を書き換える。CRC は読み飛ばされる
        png[16..20].copy_from_slice(&(MAX_SIDE + 1).to_be_bytes());
        let err = resize_bytes(&png, 10, 10, &opts(Fit::Exact, None)).unwrap_err();
        assert!(err.0 == "too_large" || err.0 == "parse", "{err:?}");
    }
}
