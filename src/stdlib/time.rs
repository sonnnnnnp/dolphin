//! `std:time`: now, monotonic, sleep, date, timestamp, format, parse, iso, parse_iso
//! （design.md 5.13）と、is_leap, days_in_month, add_months, start_of（5.30）
//!
//! 時刻は Unix 時刻（秒。小数）。暦はグレゴリオ暦で、年は 1〜9999、うるう秒は扱わない。
//! 時差は「UTC からの分」で、省略は UTC。外部クレートは使わず、暦の計算は自前

use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{error_value, int_arg, new_map, num_arg, str_arg, type_error};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:time",
        &[
            ("time.now", now),
            ("time.monotonic", monotonic),
            ("time.sleep", sleep),
            ("time.date", date),
            ("time.timestamp", timestamp),
            ("time.format", format),
            ("time.parse", parse),
            ("time.iso", iso),
            ("time.parse_iso", parse_iso),
            ("time.is_leap", is_leap_fn),
            ("time.days_in_month", days_in_month_fn),
            ("time.add_months", add_months),
            ("time.start_of", start_of),
        ],
    );
}

const MS_PER_DAY: i64 = 86_400_000;
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

// ---- 暦の計算（Howard Hinnant の civil_from_days / days_from_civil） ----

/// 1970-01-01 からの日数
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 からの日数を (年, 月, 日) にする
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 日付と時刻に分けたもの
#[derive(Debug, PartialEq)]
struct Civil {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    millisecond: i64,
    /// 0（日曜）〜6
    weekday: i64,
    /// 1 月 1 日が 1
    yday: i64,
}

/// Unix 時刻（秒）を、時差（分）をつけた日付と時刻にする
fn civil_from_ts(ts: f64, offset: i64) -> Result<Civil, String> {
    let ms = (ts * 1000.0).round();
    // 1〜9999 年に収まらない値は、ここで弾く（i64 に変換できる範囲でもある）
    if !ms.is_finite() || ms.abs() > 3.0e14 {
        return Err(format!(
            "時刻 {ts} は扱える範囲（1〜9999 年）を超えています"
        ));
    }
    let local = ms as i64 + offset * 60_000;
    let days = local.div_euclid(MS_PER_DAY);
    let in_day = local.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    if !(1..=9999).contains(&year) {
        return Err(format!(
            "時刻 {ts} は扱える範囲（1〜9999 年）を超えています"
        ));
    }
    Ok(Civil {
        year,
        month,
        day,
        hour: in_day / 3_600_000,
        minute: in_day / 60_000 % 60,
        second: in_day / 1000 % 60,
        millisecond: in_day % 1000,
        // 1970-01-01 は木曜日
        weekday: (days + 4).rem_euclid(7),
        yday: days - days_from_civil(year, 1, 1) + 1,
    })
}

/// 日付と時刻（時差つき）から Unix 時刻を作る。範囲外なら、理由を返す
fn ts_from_fields(
    (year, month, day): (i64, i64, i64),
    (hour, minute, second, ms): (i64, i64, i64, i64),
    offset: i64,
) -> Result<f64, String> {
    if !(1..=9999).contains(&year) {
        return Err(format!("年 {year} は 1〜9999 の範囲外です"));
    }
    if !(1..=12).contains(&month) {
        return Err(format!("月 {month} は 1〜12 の範囲外です"));
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return Err(format!("{year} 年 {month} 月に {day} 日はありません"));
    }
    for (name, value, max) in [("時", hour, 23), ("分", minute, 59), ("秒", second, 59)] {
        if !(0..=max).contains(&value) {
            return Err(format!("{name} {value} は 0〜{max} の範囲外です"));
        }
    }
    if !(0..=999).contains(&ms) {
        return Err(format!("ミリ秒 {ms} は 0〜999 の範囲外です"));
    }
    let total_ms = days_from_civil(year, month, day) * MS_PER_DAY
        + ((hour * 60 + minute) * 60 + second) * 1000
        + ms
        - offset * 60_000;
    Ok(total_ms as f64 / 1000.0)
}

// ---- 引数 ----

/// 引数の数が `min`〜`max` 個か確かめる
fn check_count(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), String> {
    if (min..=max).contains(&args.len()) {
        return Ok(());
    }
    let expected = if min == max {
        format!("{min} 個")
    } else {
        format!("{min} 個から {max} 個")
    };
    Err(format!(
        "{name} の引数は {expected}です（{} 個渡されました）",
        args.len()
    ))
}

/// `i` 番目の引数が時差（分）。なければ 0（UTC）
fn offset_arg(name: &str, args: &[Value], i: usize) -> Result<i64, String> {
    if args.len() <= i {
        return Ok(0);
    }
    let offset = int_arg(name, args, i)?;
    if offset.abs() >= 1440 {
        return Err(format!(
            "{name} の時差 {offset} 分は使えません（-1439〜1439）"
        ));
    }
    Ok(offset)
}

// ---- 関数 ----

/// 現在時刻（ミリ秒の精度）
fn now(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.now", args, 0, 0)?;
    let ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as f64,
        Err(before) => -(before.duration().as_millis() as f64),
    };
    Ok(Value::Num(ms / 1000.0))
}

/// 経過時間の計測用の秒。起点は不定で、戻らない
fn monotonic(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.monotonic", args, 0, 0)?;
    static START: OnceLock<Instant> = OnceLock::new();
    Ok(Value::Num(
        START.get_or_init(Instant::now).elapsed().as_secs_f64(),
    ))
}

/// 待つ。インタプリタのスレッドが止まる
fn sleep(interp: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.sleep", args, 1, 1)?;
    let seconds = num_arg("time.sleep", args, 0)?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("time.sleep の秒数は 0 以上の数です（{seconds}）"));
    }
    // 待つ前に、ここまでの出力を見せる
    interp.out.flush().map_err(|e| e.to_string())?;
    thread::sleep(Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX));
    Ok(Value::Nil)
}

fn date(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.date", args, 1, 2)?;
    let ts = num_arg("time.date", args, 0)?;
    let c = civil_from_ts(ts, offset_arg("time.date", args, 1)?)
        .map_err(|e| format!("time.date: {e}"))?;
    let mut map = Map::default();
    for (key, value) in [
        ("year", c.year),
        ("month", c.month),
        ("day", c.day),
        ("hour", c.hour),
        ("minute", c.minute),
        ("second", c.second),
        ("millisecond", c.millisecond),
        ("weekday", c.weekday),
        ("yday", c.yday),
    ] {
        map.insert(key.into(), Value::Num(value as f64));
    }
    Ok(new_map(map))
}

/// `date` の逆。範囲外は `parse` のエラー値
fn timestamp(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.timestamp", args, 1, 2)?;
    let Value::Map(fields) = &args[0] else {
        return Err(type_error("time.timestamp", 0, "マップ", &args[0]));
    };
    let fields = fields.borrow();
    const KEYS: [&str; 7] = [
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "millisecond",
    ];
    for (key, _) in fields.iter() {
        // `date` の結果をそのまま渡せるよう、`weekday` と `yday` は受け取って無視する
        if !KEYS.contains(&&**key) && !matches!(&**key, "weekday" | "yday") {
            return Err(format!(
                "time.timestamp に知らない項目 `{key}` があります（使えるのは {}）",
                KEYS.join("、")
            ));
        }
    }
    let field = |key: &str, default: Option<i64>| -> Result<i64, String> {
        match fields.get(key) {
            Some(Value::Num(n)) if n.fract() == 0.0 && n.abs() < 1e15 => Ok(*n as i64),
            Some(other) => Err(format!(
                "time.timestamp の `{key}` は整数です（{}が渡されました）",
                other.type_name()
            )),
            None => default.ok_or_else(|| format!("time.timestamp に `{key}` がありません")),
        }
    };
    let date = (
        field("year", None)?,
        field("month", None)?,
        field("day", None)?,
    );
    let time = (
        field("hour", Some(0))?,
        field("minute", Some(0))?,
        field("second", Some(0))?,
        field("millisecond", Some(0))?,
    );
    let offset = offset_arg("time.timestamp", args, 1)?;
    Ok(match ts_from_fields(date, time, offset) {
        Ok(ts) => Value::Num(ts),
        Err(message) => error_value("parse", format!("日付と時刻が正しくありません: {message}")),
    })
}

// ---- 書式 ----

fn format(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.format", args, 2, 3)?;
    let ts = num_arg("time.format", args, 0)?;
    let pattern = str_arg("time.format", args, 1)?;
    let offset = offset_arg("time.format", args, 2)?;
    let c = civil_from_ts(ts, offset).map_err(|e| format!("time.format: {e}"))?;
    let text = format_civil(&c, offset, pattern).map_err(|e| format!("time.format: {e}"))?;
    Ok(Value::Str(text.into()))
}

fn format_civil(c: &Civil, offset: i64, pattern: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(spec) = chars.next() else {
            return Err("書式が `%` で終わっています".into());
        };
        let hour12 = if c.hour % 12 == 0 { 12 } else { c.hour % 12 };
        let piece = match spec {
            'Y' => format!("{:04}", c.year),
            'y' => format!("{:02}", c.year % 100),
            'm' => format!("{:02}", c.month),
            'd' => format!("{:02}", c.day),
            'e' => format!("{:2}", c.day),
            'j' => format!("{:03}", c.yday),
            'H' => format!("{:02}", c.hour),
            'I' => format!("{hour12:02}"),
            'M' => format!("{:02}", c.minute),
            'S' => format!("{:02}", c.second),
            'f' => format!("{:03}", c.millisecond),
            'p' => (if c.hour < 12 { "AM" } else { "PM" }).to_string(),
            'a' => WEEKDAYS[c.weekday as usize][..3].to_string(),
            'A' => WEEKDAYS[c.weekday as usize].to_string(),
            'b' => MONTHS[c.month as usize - 1][..3].to_string(),
            'B' => MONTHS[c.month as usize - 1].to_string(),
            'z' => {
                let sign = if offset < 0 { '-' } else { '+' };
                format!("{sign}{:02}{:02}", offset.abs() / 60, offset.abs() % 60)
            }
            '%' => "%".to_string(),
            other => return Err(format!("書式の `%{other}` は使えません")),
        };
        out.push_str(&piece);
    }
    Ok(out)
}

/// 読み取った項目。`None` は書式に出てこなかった項目
#[derive(Default)]
struct Parsed {
    year: Option<i64>,
    year2: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
    hour: Option<i64>,
    hour12: Option<i64>,
    pm: Option<bool>,
    minute: Option<i64>,
    second: Option<i64>,
    millisecond: Option<i64>,
    offset: Option<i64>,
}

/// 入力を先頭から読み進めるカーソル
struct Cursor<'a> {
    rest: &'a str,
}

impl Cursor<'_> {
    fn eat(&mut self, expected: char) -> bool {
        if self.rest.starts_with(expected) {
            self.rest = &self.rest[expected.len_utf8()..];
            true
        } else {
            false
        }
    }

    /// ASCII の数字を `min`〜`max` 桁読む
    fn digits(&mut self, min: usize, max: usize) -> Result<i64, String> {
        let len = self
            .rest
            .bytes()
            .take(max)
            .take_while(u8::is_ascii_digit)
            .count();
        if len < min {
            return Err(format!("数字が必要です（`{}` の位置）", self.preview()));
        }
        let (digits, rest) = self.rest.split_at(len);
        self.rest = rest;
        Ok(digits.parse().expect("数字だけなので必ず数値になる"))
    }

    /// 名前の一覧から、大文字小文字を区別せずに 1 つ読む。長い名前（全部）を先に、
    /// なければ 3 文字の略称。戻り値は一覧の位置
    fn name(&mut self, names: &[&str]) -> Result<usize, String> {
        for len in [None, Some(3)] {
            for (i, name) in names.iter().enumerate() {
                let candidate = &name[..len.unwrap_or(name.len())];
                if let Some(head) = self.rest.get(..candidate.len())
                    && head.eq_ignore_ascii_case(candidate)
                {
                    self.rest = &self.rest[candidate.len()..];
                    return Ok(i);
                }
            }
        }
        Err(format!("名前が必要です（`{}` の位置）", self.preview()))
    }

    /// `Z`、`+0900`、`+09:00`、`+09` を読んで、時差（分）にする
    fn zone(&mut self) -> Result<i64, String> {
        if self.eat('Z') || self.eat('z') {
            return Ok(0);
        }
        let sign = if self.eat('+') {
            1
        } else if self.eat('-') {
            -1
        } else {
            return Err(format!(
                "時差（`Z` か `+hh:mm`）が必要です（`{}` の位置）",
                self.preview()
            ));
        };
        let hours = self.digits(2, 2)?;
        self.eat(':');
        let minutes = if self.rest.starts_with(|c: char| c.is_ascii_digit()) {
            self.digits(2, 2)?
        } else {
            0
        };
        if hours > 23 || minutes > 59 {
            return Err("時差が正しくありません".into());
        }
        Ok(sign * (hours * 60 + minutes))
    }

    /// エラーメッセージ用の、いまの位置から先の少しの文字
    fn preview(&self) -> String {
        if self.rest.is_empty() {
            "終わり".to_string()
        } else {
            self.rest.chars().take(6).collect()
        }
    }
}

/// 書式の中身が正しいか。`parse` では使えない `%j` も、ここで弾く
fn check_parse_pattern(pattern: &str) -> Result<(), String> {
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        match chars.next() {
            Some(
                'Y' | 'y' | 'm' | 'd' | 'e' | 'H' | 'I' | 'M' | 'S' | 'f' | 'p' | 'a' | 'A' | 'b'
                | 'B' | 'z' | '%',
            ) => {}
            Some('j') => return Err("書式の `%j` は parse では使えません".into()),
            Some(other) => return Err(format!("書式の `%{other}` は使えません")),
            None => return Err("書式が `%` で終わっています".into()),
        }
    }
    Ok(())
}

/// 書式に従って入力を読む。合わなければ、理由を返す
fn parse_with(input: &str, pattern: &str) -> Result<Parsed, String> {
    let mut cursor = Cursor { rest: input };
    let mut parsed = Parsed::default();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            if !cursor.eat(ch) {
                return Err(format!(
                    "`{ch}` が必要です（`{}` の位置）",
                    cursor.preview()
                ));
            }
            continue;
        }
        match chars.next().expect("書式は検査済み") {
            'Y' => parsed.year = Some(cursor.digits(4, 4)?),
            'y' => parsed.year2 = Some(cursor.digits(2, 2)?),
            'm' => parsed.month = Some(cursor.digits(1, 2)?),
            'd' => parsed.day = Some(cursor.digits(1, 2)?),
            'e' => {
                while cursor.eat(' ') {}
                parsed.day = Some(cursor.digits(1, 2)?);
            }
            'H' => parsed.hour = Some(cursor.digits(1, 2)?),
            'I' => parsed.hour12 = Some(cursor.digits(1, 2)?),
            'M' => parsed.minute = Some(cursor.digits(1, 2)?),
            'S' => parsed.second = Some(cursor.digits(1, 2)?),
            'f' => {
                let len = cursor.rest.bytes().take_while(u8::is_ascii_digit).count();
                if len == 0 {
                    return Err(format!("数字が必要です（`{}` の位置）", cursor.preview()));
                }
                // 3 桁（ミリ秒）に切り詰めるか、0 で埋める
                let frac = format!("{:0<3}", &cursor.rest[..len]);
                parsed.millisecond = Some(frac[..3].parse().expect("数字だけ"));
                cursor.rest = &cursor.rest[len..];
            }
            'p' => {
                let head = cursor.rest.get(..2).map(str::to_ascii_uppercase);
                match head.as_deref() {
                    Some("AM") => parsed.pm = Some(false),
                    Some("PM") => parsed.pm = Some(true),
                    _ => {
                        return Err(format!(
                            "`AM` か `PM` が必要です（`{}` の位置）",
                            cursor.preview()
                        ));
                    }
                }
                cursor.rest = &cursor.rest[2..];
            }
            'a' | 'A' => {
                cursor.name(&WEEKDAYS)?;
            }
            'b' | 'B' => parsed.month = Some(cursor.name(&MONTHS)? as i64 + 1),
            'z' => parsed.offset = Some(cursor.zone()?),
            '%' => {
                if !cursor.eat('%') {
                    return Err(format!("`%` が必要です（`{}` の位置）", cursor.preview()));
                }
            }
            _ => unreachable!("書式は検査済み"),
        }
    }
    if !cursor.rest.is_empty() {
        return Err(format!("余分な文字があります（`{}`）", cursor.preview()));
    }
    Ok(parsed)
}

/// 読み取った項目から Unix 時刻にする。出てこなかった項目は、年月日が 1970-01-01、時刻が 0
fn ts_from_parsed(p: &Parsed) -> Result<f64, String> {
    let year = match (p.year, p.year2) {
        (Some(y), _) => y,
        // 2 桁の年は、69〜99 が 1969〜1999、00〜68 が 2000〜2068（POSIX と同じ）
        (None, Some(y)) => {
            if y >= 69 {
                1900 + y
            } else {
                2000 + y
            }
        }
        (None, None) => 1970,
    };
    let hour = match (p.hour, p.hour12) {
        (Some(h), _) => h,
        (None, Some(h)) if (1..=12).contains(&h) => {
            h % 12 + if p.pm == Some(true) { 12 } else { 0 }
        }
        (None, Some(h)) => return Err(format!("時 {h} は 1〜12 の範囲外です")),
        (None, None) => 0,
    };
    ts_from_fields(
        (year, p.month.unwrap_or(1), p.day.unwrap_or(1)),
        (
            hour,
            p.minute.unwrap_or(0),
            p.second.unwrap_or(0),
            p.millisecond.unwrap_or(0),
        ),
        p.offset.unwrap_or(0),
    )
}

fn parse(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.parse", args, 2, 2)?;
    let input = str_arg("time.parse", args, 0)?;
    let pattern = str_arg("time.parse", args, 1)?;
    check_parse_pattern(pattern).map_err(|e| format!("time.parse: {e}"))?;
    let result = parse_with(input, pattern).and_then(|p| ts_from_parsed(&p));
    Ok(match result {
        Ok(ts) => Value::Num(ts),
        Err(message) => error_value(
            "parse",
            format!("{input:?} を `{pattern}` として読めません: {message}"),
        ),
    })
}

/// RFC 3339 の文字列にする。ミリ秒が 0 なら小数は省く
fn iso(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.iso", args, 1, 2)?;
    let ts = num_arg("time.iso", args, 0)?;
    let offset = offset_arg("time.iso", args, 1)?;
    let c = civil_from_ts(ts, offset).map_err(|e| format!("time.iso: {e}"))?;
    let fraction = if c.millisecond == 0 { "" } else { ".%f" };
    let zone = if offset == 0 { "Z" } else { "%z" };
    let pattern = format!("%Y-%m-%dT%H:%M:%S{fraction}{zone}");
    let mut text = format_civil(&c, offset, &pattern)?;
    if offset != 0 {
        // `+0900` を `+09:00` にする
        text.insert(text.len() - 2, ':');
    }
    Ok(Value::Str(text.into()))
}

/// RFC 3339 を読む。`T` の代わりに小文字の `t` か空白も許す。時差は必須
fn parse_iso(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.parse_iso", args, 1, 1)?;
    let input = str_arg("time.parse_iso", args, 0)?;
    let result = parse_iso_str(input);
    Ok(match result {
        Ok(ts) => Value::Num(ts),
        Err(message) => error_value(
            "parse",
            format!("{input:?} を RFC 3339 として読めません: {message}"),
        ),
    })
}

fn parse_iso_str(input: &str) -> Result<f64, String> {
    let mut cursor = Cursor { rest: input };
    let mut p = Parsed {
        year: Some(cursor.digits(4, 4)?),
        ..Parsed::default()
    };
    expect(&mut cursor, '-')?;
    p.month = Some(cursor.digits(2, 2)?);
    expect(&mut cursor, '-')?;
    p.day = Some(cursor.digits(2, 2)?);
    if !(cursor.eat('T') || cursor.eat('t') || cursor.eat(' ')) {
        return Err(format!("`T` が必要です（`{}` の位置）", cursor.preview()));
    }
    p.hour = Some(cursor.digits(2, 2)?);
    expect(&mut cursor, ':')?;
    p.minute = Some(cursor.digits(2, 2)?);
    expect(&mut cursor, ':')?;
    p.second = Some(cursor.digits(2, 2)?);
    if cursor.eat('.') {
        let len = cursor.rest.bytes().take_while(u8::is_ascii_digit).count();
        if len == 0 {
            return Err(format!("数字が必要です（`{}` の位置）", cursor.preview()));
        }
        let frac = format!("{:0<3}", &cursor.rest[..len]);
        p.millisecond = Some(frac[..3].parse().expect("数字だけ"));
        cursor.rest = &cursor.rest[len..];
    }
    p.offset = Some(cursor.zone()?);
    if !cursor.rest.is_empty() {
        return Err(format!("余分な文字があります（`{}`）", cursor.preview()));
    }
    ts_from_parsed(&p)
}

fn expect(cursor: &mut Cursor<'_>, ch: char) -> Result<(), String> {
    if cursor.eat(ch) {
        Ok(())
    } else {
        Err(format!(
            "`{ch}` が必要です（`{}` の位置）",
            cursor.preview()
        ))
    }
}

// ---- 日付の計算（design.md 5.30） ----

/// `i` 番目の引数が年（1〜9999 の整数）
fn year_arg(name: &str, args: &[Value], i: usize) -> Result<i64, String> {
    let year = int_arg(name, args, i)?;
    if (1..=9999).contains(&year) {
        Ok(year)
    } else {
        Err(format!("{name} の年は 1〜9999 の整数です（{year}）"))
    }
}

fn is_leap_fn(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.is_leap", args, 1, 1)?;
    Ok(Value::Bool(is_leap(year_arg("time.is_leap", args, 0)?)))
}

fn days_in_month_fn(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.days_in_month", args, 2, 2)?;
    let year = year_arg("time.days_in_month", args, 0)?;
    let month = int_arg("time.days_in_month", args, 1)?;
    if !(1..=12).contains(&month) {
        return Err(format!(
            "time.days_in_month の月は 1〜12 の整数です（{month}）"
        ));
    }
    Ok(Value::Num(days_in_month(year, month) as f64))
}

/// `n` か月後。日が月末を超えるなら月末にして、時刻は変えない
fn add_months(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.add_months", args, 2, 3)?;
    let ts = num_arg("time.add_months", args, 0)?;
    let n = int_arg("time.add_months", args, 1)?;
    let offset = offset_arg("time.add_months", args, 2)?;
    let c = civil_from_ts(ts, offset).map_err(|e| format!("time.add_months: {e}"))?;
    let total = c.year * 12 + (c.month - 1) + n;
    let (year, month) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    if !(1..=9999).contains(&year) {
        return Ok(error_value(
            "parse",
            format!("{n} か月後は扱える範囲（1〜9999 年）を超えます"),
        ));
    }
    let day = c.day.min(days_in_month(year, month));
    Ok(
        match ts_from_fields(
            (year, month, day),
            (c.hour, c.minute, c.second, c.millisecond),
            offset,
        ) {
            Ok(ts) => Value::Num(ts),
            Err(message) => error_value("parse", message),
        },
    )
}

/// `unit` の始まりの時刻
fn start_of(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    check_count("time.start_of", args, 2, 3)?;
    let ts = num_arg("time.start_of", args, 0)?;
    let unit = str_arg("time.start_of", args, 1)?;
    let offset = offset_arg("time.start_of", args, 2)?;
    let c = civil_from_ts(ts, offset).map_err(|e| format!("time.start_of: {e}"))?;
    let (date, time) = match unit {
        "year" => ((c.year, 1, 1), (0, 0, 0, 0)),
        "month" => ((c.year, c.month, 1), (0, 0, 0, 0)),
        "day" => ((c.year, c.month, c.day), (0, 0, 0, 0)),
        "hour" => ((c.year, c.month, c.day), (c.hour, 0, 0, 0)),
        "minute" => ((c.year, c.month, c.day), (c.hour, c.minute, 0, 0)),
        "second" => ((c.year, c.month, c.day), (c.hour, c.minute, c.second, 0)),
        _ => {
            return Err(format!(
                "time.start_of の単位は \"year\" \"month\" \"day\" \"hour\" \"minute\" \"second\" のどれかです（{unit:?}）"
            ));
        }
    };
    ts_from_fields(date, time, offset)
        .map(Value::Num)
        .map_err(|e| format!("time.start_of: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn civil(ts: f64) -> Civil {
        civil_from_ts(ts, 0).unwrap()
    }

    fn fmt(ts: f64, offset: i64, pattern: &str) -> String {
        format_civil(&civil_from_ts(ts, offset).unwrap(), offset, pattern).unwrap()
    }

    #[test]
    fn known_instants() {
        let c = civil(0.0);
        assert_eq!(
            (c.year, c.month, c.day, c.weekday, c.yday),
            (1970, 1, 1, 4, 1)
        );
        // 2000-02-29（うるう日）
        let c = civil(951_782_400.0);
        assert_eq!(
            (c.year, c.month, c.day, c.weekday, c.yday),
            (2000, 2, 29, 2, 60)
        );
        // 2023-11-14T22:13:20Z（火曜日）
        let c = civil(1_700_000_000.0);
        assert_eq!(
            (
                c.year, c.month, c.day, c.hour, c.minute, c.second, c.weekday
            ),
            (2023, 11, 14, 22, 13, 20, 2)
        );
    }

    #[test]
    fn range_limits() {
        let first = civil(-62_135_596_800.0);
        assert_eq!((first.year, first.month, first.day), (1, 1, 1));
        let last = civil(253_402_300_799.0);
        assert_eq!(
            (
                last.year,
                last.month,
                last.day,
                last.hour,
                last.minute,
                last.second
            ),
            (9999, 12, 31, 23, 59, 59)
        );
        assert!(civil_from_ts(-62_135_596_801.0, 0).is_err());
        assert!(civil_from_ts(253_402_300_800.0, 0).is_err());
        assert!(civil_from_ts(f64::NAN, 0).is_err());
        assert!(civil_from_ts(1e300, 0).is_err());
    }

    #[test]
    fn offsets_and_negative_times() {
        // 日本時間（+9 時間）では、日付が進む
        let c = civil_from_ts(1_700_000_000.0, 540).unwrap();
        assert_eq!((c.day, c.hour, c.minute), (15, 7, 13));
        // 1969-12-31T23:59:59.5Z
        let c = civil(-0.5);
        assert_eq!(
            (c.year, c.month, c.day, c.hour, c.second, c.millisecond),
            (1969, 12, 31, 23, 59, 500)
        );
    }

    #[test]
    fn fields_roundtrip() {
        for ts in [
            0.0,
            1.5,
            -1.0,
            951_782_400.0,
            1_700_000_000.123,
            -62_135_596_800.0,
            253_402_300_799.999,
            -2_208_988_800.0,
        ] {
            let c = civil(ts);
            let back = ts_from_fields(
                (c.year, c.month, c.day),
                (c.hour, c.minute, c.second, c.millisecond),
                0,
            )
            .unwrap();
            assert!((back - ts).abs() < 0.0005, "{ts} → {back}");
        }
        // 時差つきでも往復する
        let c = civil_from_ts(1_700_000_000.0, -300).unwrap();
        let back = ts_from_fields(
            (c.year, c.month, c.day),
            (c.hour, c.minute, c.second, c.millisecond),
            -300,
        )
        .unwrap();
        assert_eq!(back, 1_700_000_000.0);
    }

    #[test]
    fn invalid_fields() {
        let ok = (0, 0, 0, 0);
        assert!(ts_from_fields((2023, 2, 29), ok, 0).is_err());
        assert!(ts_from_fields((2024, 2, 29), ok, 0).is_ok());
        assert!(ts_from_fields((1900, 2, 29), ok, 0).is_err());
        assert!(ts_from_fields((2000, 2, 29), ok, 0).is_ok());
        assert!(ts_from_fields((2023, 13, 1), ok, 0).is_err());
        assert!(ts_from_fields((0, 1, 1), ok, 0).is_err());
        assert!(ts_from_fields((2023, 1, 1), (24, 0, 0, 0), 0).is_err());
    }

    #[test]
    fn formatting() {
        let ts = 1_700_000_000.5;
        assert_eq!(
            fmt(ts, 0, "%Y-%m-%d %H:%M:%S.%f"),
            "2023-11-14 22:13:20.500"
        );
        assert_eq!(
            fmt(ts, 0, "%a %A %b %B %e %j %y %p %I %%"),
            "Tue Tuesday Nov November 14 318 23 PM 10 %"
        );
        assert_eq!(fmt(ts, 540, "%H:%M %z"), "07:13 +0900");
        assert_eq!(fmt(ts, -330, "%z"), "-0530");
        assert_eq!(fmt(0.0, 0, "%I %p"), "12 AM");
        assert_eq!(fmt(43_200.0, 0, "%I %p"), "12 PM");
        assert_eq!(fmt(0.0, 0, "%e"), " 1");
        assert!(format_civil(&civil(0.0), 0, "%Q").is_err());
        assert!(format_civil(&civil(0.0), 0, "%").is_err());
    }

    fn parsed(input: &str, pattern: &str) -> Result<f64, String> {
        check_parse_pattern(pattern)?;
        parse_with(input, pattern).and_then(|p| ts_from_parsed(&p))
    }

    #[test]
    fn parsing() {
        assert_eq!(
            parsed("2023-11-14 22:13:20", "%Y-%m-%d %H:%M:%S"),
            Ok(1_700_000_000.0)
        );
        assert_eq!(
            parsed("2023-11-14 22:13:20 +0900", "%Y-%m-%d %H:%M:%S %z"),
            Ok(1_700_000_000.0 - 32_400.0)
        );
        assert_eq!(parsed("14 nov 2023", "%d %b %Y"), Ok(1_699_920_000.0));
        assert_eq!(
            parsed("Tuesday, November 14, 2023", "%A, %B %d, %Y"),
            Ok(1_699_920_000.0)
        );
        assert_eq!(parsed("10:13:20.5 PM", "%I:%M:%S.%f %p"), Ok(80_000.5));
        assert_eq!(parsed("12:00 AM", "%I:%M %p"), Ok(0.0));
        assert_eq!(parsed("12:00 PM", "%I:%M %p"), Ok(43_200.0));
        assert_eq!(parsed(" 5", "%e"), Ok(345_600.0));
        // 年がなければ 1970 年
        assert_eq!(parsed("01-02", "%m-%d"), Ok(86_400.0));
        // 2 桁の年の境目
        assert_eq!(parsed("68", "%y"), parsed("2068", "%Y"));
        assert_eq!(parsed("69", "%y"), parsed("1969", "%Y"));
        // %f は 3 桁に切り詰める・埋める
        assert_eq!(parsed("1", "%f"), Ok(0.1));
        assert_eq!(parsed("12345", "%f"), Ok(0.123));
    }

    #[test]
    fn parse_failures() {
        assert!(parsed("2023-13-01", "%Y-%m-%d").is_err());
        assert!(parsed("2023-02-30", "%Y-%m-%d").is_err());
        assert!(parsed("2023-11", "%Y-%m-%d").is_err());
        assert!(parsed("2023-11-14x", "%Y-%m-%d").is_err());
        assert!(parsed("x", "%Y").is_err());
        assert!(parsed("13:00 PM", "%I:%M %p").is_err());
        assert!(parsed("2023", "%j").is_err());
        assert!(parsed("1", "%Q").is_err());
    }

    #[test]
    fn iso_roundtrip() {
        assert_eq!(parse_iso_str("2023-11-14T22:13:20Z"), Ok(1_700_000_000.0));
        assert_eq!(parse_iso_str("2023-11-14t22:13:20z"), Ok(1_700_000_000.0));
        assert_eq!(parse_iso_str("2023-11-14 22:13:20Z"), Ok(1_700_000_000.0));
        assert_eq!(
            parse_iso_str("2023-11-15T07:13:20+09:00"),
            Ok(1_700_000_000.0)
        );
        assert_eq!(
            parse_iso_str("2023-11-14T22:13:20.5-00:00"),
            Ok(1_700_000_000.5)
        );
        assert_eq!(
            parse_iso_str("2023-11-14T22:13:20.123456Z"),
            Ok(1_700_000_000.123)
        );
        assert!(parse_iso_str("2023-11-14T22:13:20").is_err());
        assert!(parse_iso_str("2023-11-14").is_err());
        assert!(parse_iso_str("2023-11-14T22:13:20+9:00").is_err());
        assert!(parse_iso_str("2023-11-14T25:00:00Z").is_err());
        assert!(parse_iso_str("2023-11-14T22:13:20Zx").is_err());
    }
}
