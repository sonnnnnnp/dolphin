//! `std:sqlite`: open, close, exec, query, run_script（design.md 5.28）
//!
//! `rusqlite`（SQLite のソースを同梱。`sqlite` feature）を使う

use std::rc::Rc;
use std::time::Duration;

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, ErrorCode, OpenFlags, Statement};

use super::{error_value, expect_args, new_array, new_map, str_arg, type_error};
use crate::runtime::handle::{Handle, HandleKind, SqliteConn};
use crate::runtime::interp::Interpreter;
use crate::runtime::map::Map;
use crate::runtime::value::Value;

/// ロックされているときに待つ時間の既定（秒）
const DEFAULT_TIMEOUT: f64 = 5.0;
/// 浮動小数点数のまま正確に表せる整数の上限
const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:sqlite",
        &[
            ("sqlite.open", open),
            ("sqlite.close", close),
            ("sqlite.exec", exec),
            ("sqlite.query", query),
            ("sqlite.run_script", run_script),
        ],
    );
}

// ---- 引数・エラー ----

fn handle_arg<'v>(name: &str, args: &'v [Value], i: usize) -> Result<&'v Rc<Handle>, String> {
    match &args[i] {
        Value::Handle(h) => Ok(h),
        other => Err(type_error(name, i, "ハンドル", other)),
    }
}

fn closed() -> Value {
    error_value("closed", "接続はすでに閉じています")
}

/// rusqlite のエラーをエラー値にする。`open` のときは、開けない失敗を `io` にする
fn failure(what: &str, e: &rusqlite::Error) -> Value {
    let kind = match e {
        rusqlite::Error::SqliteFailure(err, _) => match err.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => "busy",
            ErrorCode::CannotOpen
            | ErrorCode::SystemIoFailure
            | ErrorCode::DiskFull
            | ErrorCode::NotADatabase => "io",
            _ => "sql",
        },
        _ => "sql",
    };
    error_value(kind, format!("{what}: {e}"))
}

/// 接続に `f` を適用する。閉じていればエラー値、SQLite でないハンドルなら実行時エラー
fn with_conn(
    name: &str,
    args: &[Value],
    f: impl FnOnce(&Connection) -> Result<Value, String>,
) -> Result<Value, String> {
    let handle = handle_arg(name, args, 0)?;
    let inner = handle.inner.borrow();
    match &*inner {
        HandleKind::Sqlite(db) => f(&db.conn),
        HandleKind::SqliteClosed => Ok(closed()),
        _ => Err(format!(
            "{name} に渡せるのは SQLite の接続のハンドルです（{handle}でした）"
        )),
    }
}

// ---- open / close ----

fn open(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if !(1..=2).contains(&args.len()) {
        return Err(format!(
            "sqlite.open の引数は 1 個か 2 個です（{} 個渡されました）",
            args.len()
        ));
    }
    let path = str_arg("sqlite.open", args, 0)?;
    let mut read_only = false;
    let mut timeout = DEFAULT_TIMEOUT;
    if let Some(options) = args.get(1) {
        let Value::Map(options) = options else {
            return Err(type_error("sqlite.open", 1, "マップ", options));
        };
        for (key, value) in options.borrow().iter() {
            match (&**key, value) {
                ("read_only", Value::Bool(b)) => read_only = *b,
                ("timeout", Value::Num(n)) => {
                    if !(n.is_finite() && *n >= 0.0) {
                        return Err(format!("sqlite.open の `timeout` は 0 以上の数です（{n}）"));
                    }
                    timeout = *n;
                }
                ("read_only" | "timeout", other) => {
                    return Err(format!(
                        "sqlite.open のオプション `{key}` の型が違います（{}が渡されました）",
                        other.type_name()
                    ));
                }
                _ => return Err(format!("sqlite.open のオプション `{key}` は知りません")),
            }
        }
    }
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match Connection::open_with_flags(path, flags) {
        Ok(conn) => conn,
        Err(e) => return Ok(failure(&format!("{path} を開けません"), &e)),
    };
    if let Err(e) = conn.busy_timeout(Duration::from_secs_f64(timeout)) {
        return Ok(failure("待ち時間を設定できません", &e));
    }
    Ok(Value::Handle(Handle::new(HandleKind::Sqlite(SqliteConn {
        conn,
        path: path.to_string(),
    }))))
}

fn close(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("sqlite.close", args, 1)?;
    let handle = handle_arg("sqlite.close", args, 0)?;
    if !matches!(
        &*handle.inner.borrow(),
        HandleKind::Sqlite(_) | HandleKind::SqliteClosed
    ) {
        return Err(format!(
            "sqlite.close に渡せるのは SQLite の接続のハンドルです（{handle}でした）"
        ));
    }
    handle.close();
    Ok(Value::Nil)
}

// ---- 値の変換 ----

/// Dolphin の値を SQLite の値にする（design.md 5.28）。変換できない型は実行時エラー
fn to_sql(name: &str, value: &Value) -> Result<SqlValue, String> {
    Ok(match value {
        Value::Nil => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Num(n) if n.fract() == 0.0 && n.abs() <= MAX_EXACT => SqlValue::Integer(*n as i64),
        Value::Num(n) => SqlValue::Real(*n),
        Value::Str(s) => SqlValue::Text(s.to_string()),
        Value::Bytes(b) => SqlValue::Blob(b.to_vec()),
        other => {
            return Err(format!(
                "{name} のパラメータに渡せるのは nil・真偽値・数値・文字列・バイト列です（{}が渡されました）",
                other.type_name()
            ));
        }
    })
}

fn from_sql(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Nil,
        ValueRef::Integer(i) => Value::Num(i as f64),
        ValueRef::Real(r) => Value::Num(r),
        ValueRef::Text(t) => Value::Str(String::from_utf8_lossy(t).into_owned().into()),
        ValueRef::Blob(b) => Value::bytes(b),
    }
}

// ---- 文の準備と実行 ----

/// パラメータを束ねる。配列は `?` に順番に、マップは `:name` `@name` `$name` に名前で入れる。
/// 数や名前が合わなければエラー値
fn bind(
    name: &str,
    stmt: &mut Statement<'_>,
    params: Option<&Value>,
) -> Result<Option<Value>, String> {
    let count = stmt.parameter_count();
    match params {
        None | Some(Value::Nil) => {
            if count != 0 {
                return Ok(Some(error_value(
                    "sql",
                    format!("パラメータの数が合いません（必要 {count} 個、渡されたのは 0 個）"),
                )));
            }
        }
        Some(Value::Array(items)) => {
            let items = items.borrow();
            if items.len() != count {
                return Ok(Some(error_value(
                    "sql",
                    format!(
                        "パラメータの数が合いません（必要 {count} 個、渡されたのは {} 個）",
                        items.len()
                    ),
                )));
            }
            for (i, item) in items.iter().enumerate() {
                let value = to_sql(name, item)?;
                if let Err(e) = stmt.raw_bind_parameter(i + 1, value) {
                    return Ok(Some(failure("パラメータを入れられません", &e)));
                }
            }
        }
        Some(Value::Map(map)) => {
            let mut bound = vec![false; count];
            for (key, item) in map.borrow().iter() {
                let index = [":", "@", "$"].iter().find_map(|prefix| {
                    stmt.parameter_index(&format!("{prefix}{key}"))
                        .ok()
                        .flatten()
                });
                let Some(index) = index else {
                    return Ok(Some(error_value(
                        "sql",
                        format!("名前 `{key}` のパラメータはありません"),
                    )));
                };
                let value = to_sql(name, item)?;
                if let Err(e) = stmt.raw_bind_parameter(index, value) {
                    return Ok(Some(failure("パラメータを入れられません", &e)));
                }
                bound[index - 1] = true;
            }
            if let Some(missing) = bound.iter().position(|b| !b) {
                let what = stmt
                    .parameter_name(missing + 1)
                    .map_or_else(|| "?".to_string(), str::to_string);
                return Ok(Some(error_value(
                    "sql",
                    format!("パラメータ `{what}` の値がありません"),
                )));
            }
        }
        Some(other) => return Err(type_error(name, 2, "配列かマップ", other)),
    }
    Ok(None)
}

fn sql_args<'v>(name: &str, args: &'v [Value]) -> Result<(&'v str, Option<&'v Value>), String> {
    if !(2..=3).contains(&args.len()) {
        return Err(format!(
            "{name} の引数は 2 個か 3 個です（{} 個渡されました）",
            args.len()
        ));
    }
    Ok((str_arg(name, args, 1)?, args.get(2)))
}

fn exec(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (sql, params) = sql_args("sqlite.exec", args)?;
    with_conn("sqlite.exec", args, |conn| {
        let mut stmt = match conn.prepare(sql) {
            Ok(stmt) => stmt,
            Err(e) => return Ok(failure("SQL を準備できません", &e)),
        };
        if let Some(error) = bind("sqlite.exec", &mut stmt, params)? {
            return Ok(error);
        }
        // 結果の行がある文でも実行できるように、行を読み捨てる
        let mut rows = stmt.raw_query();
        loop {
            match rows.next() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => return Ok(failure("SQL を実行できません", &e)),
            }
        }
        drop(rows);
        let changes = if stmt.readonly() { 0 } else { conn.changes() };
        let mut result = Map::default();
        result.insert("changes".into(), Value::Num(changes as f64));
        result.insert(
            "last_id".into(),
            Value::Num(conn.last_insert_rowid() as f64),
        );
        Ok(new_map(result))
    })
}

fn query(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    let (sql, params) = sql_args("sqlite.query", args)?;
    with_conn("sqlite.query", args, |conn| {
        let mut stmt = match conn.prepare(sql) {
            Ok(stmt) => stmt,
            Err(e) => return Ok(failure("SQL を準備できません", &e)),
        };
        if let Some(error) = bind("sqlite.query", &mut stmt, params)? {
            return Ok(error);
        }
        let columns: Vec<Rc<str>> = stmt.column_names().into_iter().map(Rc::from).collect();
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        loop {
            let row = match rows.next() {
                Ok(Some(row)) => row,
                Ok(None) => break,
                Err(e) => return Ok(failure("SQL を実行できません", &e)),
            };
            let mut map = Map::default();
            for (i, name) in columns.iter().enumerate() {
                match row.get_ref(i) {
                    Ok(value) => map.insert(Rc::clone(name), from_sql(value)),
                    Err(e) => return Ok(failure("結果を読めません", &e)),
                }
            }
            out.push(new_map(map));
        }
        Ok(new_array(out))
    })
}

fn run_script(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("sqlite.run_script", args, 2)?;
    let sql = str_arg("sqlite.run_script", args, 1)?;
    with_conn("sqlite.run_script", args, |conn| {
        Ok(match conn.execute_batch(sql) {
            Ok(()) => Value::Nil,
            Err(e) => failure("SQL を実行できません", &e),
        })
    })
}
