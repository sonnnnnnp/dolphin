//! `std:ffi`: C の共有ライブラリの関数を呼ぶ（design.md 5.44）。`ffi` feature。
//!
//! 共有ライブラリを開くのに `libloading`、呼び出し規約に `libffi` を使う。
//! 間違った型や不正なアドレスを渡すと、プロセスごと落ちることがある（FFI はそういうもの）

use std::any::Any;
use std::ffi::{CStr, CString, c_void};
use std::rc::Rc;

use libffi::middle::{Arg, Cif, CodePtr, Type};
use libloading::Library;

use super::{bytes_arg, error_value, expect_args, int_arg, str_arg, type_error};
use crate::runtime::handle::{Handle, HandleKind};
use crate::runtime::interp::Interpreter;
use crate::runtime::value::Value;

/// バッファの大きさの上限
const MAX_BUFFER: i64 = 64 * 1024 * 1024;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module(
        "std:ffi",
        &[
            ("ffi.open", open),
            ("ffi.func", func),
            ("ffi.call", call),
            ("ffi.buffer", buffer),
            ("ffi.buffer_from", buffer_from),
            ("ffi.read", read),
            ("ffi.write", write),
            ("ffi.size", size),
            ("ffi.address", address),
            ("ffi.peek", peek),
            ("ffi.cstr", cstr),
        ],
    );
}

// ---- 型 ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Void,
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Ptr,
    Str,
    Bytes,
    Buf,
}

impl Ty {
    fn parse(name: &str, text: &str, is_ret: bool) -> Result<Ty, String> {
        let ty = match text {
            "void" => Ty::Void,
            "bool" => Ty::Bool,
            "i8" => Ty::I8,
            "i16" => Ty::I16,
            "i32" => Ty::I32,
            "i64" => Ty::I64,
            "u8" => Ty::U8,
            "u16" => Ty::U16,
            "u32" => Ty::U32,
            "u64" => Ty::U64,
            "f32" => Ty::F32,
            "f64" => Ty::F64,
            "ptr" => Ty::Ptr,
            "str" => Ty::Str,
            "bytes" => Ty::Bytes,
            "buf" => Ty::Buf,
            other => {
                return Err(format!(
                    "{name} の型 {other:?} は知りません（void bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 ptr str bytes buf）"
                ));
            }
        };
        match (ty, is_ret) {
            (Ty::Void, false) => Err(format!("{name} の引数に `void` は使えません")),
            (Ty::Bytes | Ty::Buf, true) => Err(format!(
                "{name} の戻り値に `{text}` は使えません（`bytes` と `buf` は引数だけです）"
            )),
            _ => Ok(ty),
        }
    }

    fn libffi(self) -> Type {
        match self {
            Ty::Void => Type::void(),
            Ty::Bool | Ty::U8 => Type::u8(),
            Ty::I8 => Type::i8(),
            Ty::I16 => Type::i16(),
            Ty::I32 => Type::i32(),
            Ty::I64 => Type::i64(),
            Ty::U16 => Type::u16(),
            Ty::U32 => Type::u32(),
            Ty::U64 => Type::u64(),
            Ty::F32 => Type::f32(),
            Ty::F64 => Type::f64(),
            Ty::Ptr | Ty::Str | Ty::Bytes | Ty::Buf => Type::pointer(),
        }
    }
}

// ---- ハンドルの中身 ----

struct Lib {
    lib: Rc<Library>,
}

struct Func {
    /// 関数のあるライブラリを開いたままにする
    _lib: Rc<Library>,
    addr: usize,
    cif: Cif,
    args: Vec<Ty>,
    ret: Ty,
}

struct Buffer {
    data: Box<[u8]>,
}

fn ffi_handle(object: impl Any, label: String) -> Value {
    Value::Handle(Handle::new(HandleKind::Ffi {
        object: Box::new(object),
        label,
    }))
}

/// `value` が `T` のハンドルなら `f` を適用する。閉じていればエラー値、別の種類なら実行時エラー
fn with_object<T: Any, R>(
    name: &str,
    value: &Value,
    i: usize,
    what: &str,
    f: impl FnOnce(&mut T) -> Result<R, String>,
) -> Result<Result<R, Value>, String> {
    let Value::Handle(handle) = value else {
        return Err(type_error(name, i, what, value));
    };
    let mut inner = handle.inner.borrow_mut();
    match &mut *inner {
        HandleKind::Ffi { object, .. } => match object.downcast_mut::<T>() {
            Some(object) => f(object).map(Ok),
            None => Err(format!(
                "{name} の {} 番目の引数は{what}です（FFI の別の種類のハンドルでした）",
                i + 1
            )),
        },
        HandleKind::Closed => Ok(Err(super::net::closed())),
        _ => Err(format!(
            "{name} の {} 番目の引数は{what}です（ほかの種類のハンドルでした）",
            i + 1
        )),
    }
}

/// `with_object` の結果を、関数の戻り値にする（エラー値はそのまま返す）
fn flatten(result: Result<Result<Value, Value>, String>) -> Result<Value, String> {
    result.map(|inner| inner.unwrap_or_else(|error| error))
}

// ---- ライブラリと関数 ----

fn open_failure(path: &str, e: &libloading::Error) -> Value {
    // libloading のエラーの中身は読めないので、見つからなかったかは文面と、パスの有無で見分ける
    // （OS の文面は言語によって違うので、Windows では「ファイルがなければ見つからない」とする）
    let text = e.to_string().to_lowercase();
    let not_found = !std::path::Path::new(path).exists()
        && (cfg!(windows)
            || text.contains("no such file")
            || text.contains("cannot open shared object")
            || text.contains("image not found"));
    let kind = if not_found { "not_found" } else { "io" };
    error_value(kind, format!("`{path}` を開けません: {e}"))
}

fn open(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.open", args, 1)?;
    let path = str_arg("ffi.open", args, 0)?;
    if path.is_empty() || path.contains('\0') {
        return Err("ffi.open のパスが正しくありません".into());
    }
    // 読み込み時に、ライブラリの初期化コードが走る。それは呼ぶ側の責任
    Ok(match unsafe { Library::new(path) } {
        Ok(lib) => ffi_handle(Lib { lib: Rc::new(lib) }, format!("ライブラリ {path}")),
        Err(e) => open_failure(path, &e),
    })
}

fn func(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.func", args, 4)?;
    let name = str_arg("ffi.func", args, 1)?;
    let Value::Array(types) = &args[2] else {
        return Err(type_error("ffi.func", 2, "型名の配列", &args[2]));
    };
    let mut arg_types = Vec::new();
    for item in types.borrow().iter() {
        let Value::Str(text) = item else {
            return Err(format!(
                "ffi.func の引数の型は文字列です（{}が渡されました）",
                item.type_name()
            ));
        };
        arg_types.push(Ty::parse("ffi.func", text, false)?);
    }
    let ret = Ty::parse("ffi.func", str_arg("ffi.func", args, 3)?, true)?;
    if name.is_empty() || name.contains('\0') {
        return Err("ffi.func のシンボル名が正しくありません".into());
    }

    let looked_up = with_object::<Lib, _>("ffi.func", &args[0], 0, "ライブラリ", |lib| {
        // シンボルの型は、アドレスを取り出すためだけに決める
        let symbol = unsafe { lib.lib.get::<unsafe extern "C" fn()>(name.as_bytes()) };
        Ok(match symbol {
            Ok(symbol) => Ok((Rc::clone(&lib.lib), *symbol as usize)),
            Err(e) => Err(error_value(
                "not_found",
                format!("シンボル `{name}` が見つかりません: {e}"),
            )),
        })
    })?;
    let (lib, addr) = match looked_up {
        Ok(Ok(found)) => found,
        Ok(Err(error)) | Err(error) => return Ok(error),
    };
    let cif = Cif::new(arg_types.iter().map(|t| t.libffi()), ret.libffi());
    Ok(ffi_handle(
        Func {
            _lib: lib,
            addr,
            cif,
            args: arg_types,
            ret,
        },
        format!("関数 {name}"),
    ))
}

// ---- 呼ぶ ----

/// 渡す値。`Arg` はこれへの参照を取る
enum Slot {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Ptr(*const c_void),
}

impl Slot {
    fn arg(&self) -> Arg<'_> {
        match self {
            Slot::I8(v) => Arg::new(v),
            Slot::I16(v) => Arg::new(v),
            Slot::I32(v) => Arg::new(v),
            Slot::I64(v) => Arg::new(v),
            Slot::U8(v) => Arg::new(v),
            Slot::U16(v) => Arg::new(v),
            Slot::U32(v) => Arg::new(v),
            Slot::U64(v) => Arg::new(v),
            Slot::F32(v) => Arg::new(v),
            Slot::F64(v) => Arg::new(v),
            Slot::Ptr(v) => Arg::new(v),
        }
    }
}

/// 整数型の引数の値。数値（整数の値）か整数
fn integer(name: &str, i: usize, value: &Value) -> Result<i128, String> {
    match value {
        Value::Int(n) => Ok(i128::from(*n)),
        Value::Num(n) if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0 => Ok(*n as i128),
        Value::Num(n) => Err(format!(
            "{name} の {i} 番目の引数は整数の値です（{n} が渡されました）"
        )),
        other => Err(format!(
            "{name} の {i} 番目の引数は数値か整数です（{}が渡されました）",
            other.type_name()
        )),
    }
}

fn in_range<T: TryFrom<i128>>(name: &str, i: usize, ty: &str, n: i128) -> Result<T, String> {
    T::try_from(n).map_err(|_| format!("{name} の {i} 番目の引数 {n} は {ty} の範囲を出ています"))
}

fn float(name: &str, i: usize, value: &Value) -> Result<f64, String> {
    match value {
        Value::Num(n) => Ok(*n),
        Value::Int(n) => Ok(*n as f64),
        other => Err(format!(
            "{name} の {i} 番目の引数は数値です（{}が渡されました）",
            other.type_name()
        )),
    }
}

/// 呼ぶ間、渡したポインタの先を生かしておくもの
#[derive(Default)]
struct Keep {
    strings: Vec<CString>,
    bytes: Vec<Vec<u8>>,
}

fn to_slot(name: &str, i: usize, ty: Ty, value: &Value, keep: &mut Keep) -> Result<Slot, String> {
    Ok(match ty {
        Ty::Void => unreachable!("引数に void は登録できない"),
        Ty::Bool => match value {
            Value::Bool(b) => Slot::U8(u8::from(*b)),
            other => {
                return Err(format!(
                    "{name} の {i} 番目の引数は真偽値です（{}が渡されました）",
                    other.type_name()
                ));
            }
        },
        Ty::I8 => Slot::I8(in_range(name, i, "i8", integer(name, i, value)?)?),
        Ty::I16 => Slot::I16(in_range(name, i, "i16", integer(name, i, value)?)?),
        Ty::I32 => Slot::I32(in_range(name, i, "i32", integer(name, i, value)?)?),
        Ty::I64 => Slot::I64(in_range(name, i, "i64", integer(name, i, value)?)?),
        Ty::U8 => Slot::U8(in_range(name, i, "u8", integer(name, i, value)?)?),
        Ty::U16 => Slot::U16(in_range(name, i, "u16", integer(name, i, value)?)?),
        Ty::U32 => Slot::U32(in_range(name, i, "u32", integer(name, i, value)?)?),
        Ty::U64 => Slot::U64(in_range(name, i, "u64", integer(name, i, value)?)?),
        Ty::F32 => Slot::F32(float(name, i, value)? as f32),
        Ty::F64 => Slot::F64(float(name, i, value)?),
        Ty::Ptr => match value {
            Value::Nil => Slot::Ptr(std::ptr::null()),
            // アドレスは、ビットをそのまま整数にしたもの
            Value::Int(n) => Slot::Ptr(*n as usize as *const c_void),
            Value::Num(_) => Slot::Ptr(integer(name, i, value)? as usize as *const c_void),
            other => {
                return Err(format!(
                    "{name} の {i} 番目の引数は、アドレス（整数）か nil です（{}が渡されました）",
                    other.type_name()
                ));
            }
        },
        Ty::Str => match value {
            Value::Nil => Slot::Ptr(std::ptr::null()),
            Value::Str(s) => {
                let c = CString::new(s.as_bytes()).map_err(|_| {
                    format!("{name} の {i} 番目の引数の文字列に NUL は含められません")
                })?;
                let ptr = c.as_ptr().cast::<c_void>();
                keep.strings.push(c);
                Slot::Ptr(ptr)
            }
            other => {
                return Err(format!(
                    "{name} の {i} 番目の引数は文字列か nil です（{}が渡されました）",
                    other.type_name()
                ));
            }
        },
        Ty::Bytes => match value {
            Value::Bytes(b) => {
                let mut copy = b.to_vec();
                // 空でも有効なアドレスを渡す
                copy.push(0);
                let ptr = copy.as_ptr().cast::<c_void>();
                keep.bytes.push(copy);
                Slot::Ptr(ptr)
            }
            Value::Nil => Slot::Ptr(std::ptr::null()),
            other => {
                return Err(format!(
                    "{name} の {i} 番目の引数はバイト列か nil です（{}が渡されました）",
                    other.type_name()
                ));
            }
        },
        Ty::Buf => {
            let found = with_object::<Buffer, _>(name, value, i - 1, "バッファ", |buf| {
                Ok(buf.data.as_mut_ptr().cast::<c_void>().cast_const())
            })?;
            match found {
                Ok(ptr) => Slot::Ptr(ptr),
                Err(_) => {
                    return Err(format!("{name} の {i} 番目の引数のバッファは閉じています"));
                }
            }
        }
    })
}

/// C の文字列を読む。NULL は `nil`、UTF-8 でなければ `parse` のエラー値
unsafe fn read_cstr(ptr: *const c_void) -> Value {
    if ptr.is_null() {
        return Value::Nil;
    }
    // SAFETY: 呼ぶ側（`ffi.cstr` / 戻り値の `str`）が、NUL 終端の文字列を指すと約束している
    let text = unsafe { CStr::from_ptr(ptr.cast()) };
    match text.to_str() {
        Ok(s) => Value::Str(s.into()),
        Err(_) => error_value("parse", "C の文字列が UTF-8 ではありません"),
    }
}

fn call(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.call", args, 2)?;
    let Value::Array(list) = &args[1] else {
        return Err(type_error("ffi.call", 1, "引数の配列", &args[1]));
    };
    let values = list.borrow().clone();
    // 関数のハンドルから、必要な情報だけ取り出す（C を呼ぶ間はハンドルを借りたままにしない）
    let signature =
        with_object::<Func, _>("ffi.call", &args[0], 0, "関数", |f| Ok(f.args.clone()))?;
    let types = match signature {
        Ok(found) => found,
        Err(error) => return Ok(error),
    };
    if values.len() != types.len() {
        return Err(format!(
            "ffi.call の引数の配列は {} 個です（{} 個渡されました）",
            types.len(),
            values.len()
        ));
    }
    let mut keep = Keep::default();
    let mut slots = Vec::with_capacity(types.len());
    for (i, (ty, value)) in types.iter().zip(&values).enumerate() {
        slots.push(to_slot("ffi.call", i + 1, *ty, value, &mut keep)?);
    }
    let call_args: Vec<Arg> = slots.iter().map(Slot::arg).collect();

    let result = with_object::<Func, _>("ffi.call", &args[0], 0, "関数", |f| {
        let code = CodePtr(f.addr as *mut c_void);
        // SAFETY: 登録した型の組どおりに引数を作った。関数が本当にその型かは、登録した人の責任
        let value = unsafe {
            match f.ret {
                Ty::Void => {
                    f.cif.call::<()>(code, &call_args);
                    Value::Nil
                }
                // 小さい整数の戻り値は `ffi_arg` の大きさで書かれるので、広い型で受けてから縮める
                Ty::Bool => Value::Bool(f.cif.call::<u64>(code, &call_args) as u8 != 0),
                Ty::I8 => Value::Num(f64::from(f.cif.call::<i64>(code, &call_args) as i8)),
                Ty::I16 => Value::Num(f64::from(f.cif.call::<i64>(code, &call_args) as i16)),
                Ty::I32 => Value::Num(f64::from(f.cif.call::<i64>(code, &call_args) as i32)),
                Ty::U8 => Value::Num(f64::from(f.cif.call::<u64>(code, &call_args) as u8)),
                Ty::U16 => Value::Num(f64::from(f.cif.call::<u64>(code, &call_args) as u16)),
                Ty::U32 => Value::Num(f64::from(f.cif.call::<u64>(code, &call_args) as u32)),
                Ty::I64 => Value::Int(f.cif.call::<i64>(code, &call_args)),
                Ty::U64 => {
                    let n = f.cif.call::<u64>(code, &call_args);
                    match i64::try_from(n) {
                        Ok(n) => Value::Int(n),
                        Err(_) => {
                            return Err(format!(
                                "ffi.call の戻り値 {n} は整数（i64）の範囲を出ています"
                            ));
                        }
                    }
                }
                Ty::F32 => Value::Num(f64::from(f.cif.call::<f32>(code, &call_args))),
                Ty::F64 => Value::Num(f.cif.call::<f64>(code, &call_args)),
                Ty::Ptr => {
                    let ptr = f.cif.call::<usize>(code, &call_args);
                    if ptr == 0 {
                        Value::Nil
                    } else {
                        Value::Int(ptr as i64)
                    }
                }
                Ty::Str => {
                    let ptr = f.cif.call::<usize>(code, &call_args);
                    read_cstr(ptr as *const c_void)
                }
                Ty::Bytes | Ty::Buf => unreachable!("戻り値に bytes と buf は登録できない"),
            }
        };
        Ok(value)
    })?;
    // 渡したポインタの先は、呼び終わるまで生きている
    drop(keep);
    Ok(result.unwrap_or_else(|error| error))
}

// ---- バッファ ----

fn buffer(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.buffer", args, 1)?;
    let n = int_arg("ffi.buffer", args, 0)?;
    if !(0..=MAX_BUFFER).contains(&n) {
        return Err(format!("ffi.buffer の大きさは 0〜{MAX_BUFFER} です（{n}）"));
    }
    Ok(new_buffer(vec![0; n as usize]))
}

fn buffer_from(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.buffer_from", args, 1)?;
    let bytes = bytes_arg("ffi.buffer_from", args, 0)?;
    Ok(new_buffer(bytes.to_vec()))
}

fn new_buffer(data: Vec<u8>) -> Value {
    let label = format!("バッファ {} バイト", data.len());
    ffi_handle(
        Buffer {
            data: data.into_boxed_slice(),
        },
        label,
    )
}

/// `offset` から `n` バイトの範囲が、大きさ `len` に収まるか
fn range_arg(name: &str, len: usize, n: i64, offset: i64) -> Result<usize, String> {
    if offset < 0 || n < 0 || (offset as u128 + n as u128) > len as u128 {
        return Err(format!(
            "{name} の範囲（offset {offset}、{n} バイト）がバッファの大きさ {len} を出ています"
        ));
    }
    Ok(offset as usize)
}

fn read(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.read", args, 3)?;
    let (offset, n) = (int_arg("ffi.read", args, 1)?, int_arg("ffi.read", args, 2)?);
    flatten(with_object::<Buffer, _>(
        "ffi.read",
        &args[0],
        0,
        "バッファ",
        |buf| {
            let start = range_arg("ffi.read", buf.data.len(), n, offset)?;
            Ok(Value::bytes(&buf.data[start..start + n as usize]))
        },
    ))
}

fn write(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.write", args, 3)?;
    let offset = int_arg("ffi.write", args, 1)?;
    let bytes = bytes_arg("ffi.write", args, 2)?;
    flatten(with_object::<Buffer, _>(
        "ffi.write",
        &args[0],
        0,
        "バッファ",
        |buf| {
            let n = bytes.len() as i64;
            let start = range_arg("ffi.write", buf.data.len(), n, offset)?;
            buf.data[start..start + bytes.len()].copy_from_slice(bytes);
            Ok(Value::Nil)
        },
    ))
}

fn size(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.size", args, 1)?;
    flatten(with_object::<Buffer, _>(
        "ffi.size",
        &args[0],
        0,
        "バッファ",
        |buf| Ok(Value::Num(buf.data.len() as f64)),
    ))
}

fn address(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.address", args, 1)?;
    flatten(with_object::<Buffer, _>(
        "ffi.address",
        &args[0],
        0,
        "バッファ",
        |buf| Ok(Value::Int(buf.data.as_ptr() as usize as i64)),
    ))
}

// ---- 生のアドレス ----

fn address_arg(name: &str, args: &[Value], i: usize) -> Result<usize, String> {
    match &args[i] {
        Value::Int(n) => Ok(*n as usize),
        Value::Num(_) => Ok(integer(name, i + 1, &args[i])? as usize),
        other => Err(type_error(name, i, "アドレス（整数）", other)),
    }
}

fn peek(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.peek", args, 2)?;
    let addr = address_arg("ffi.peek", args, 0)?;
    let n = int_arg("ffi.peek", args, 1)?;
    if !(0..=MAX_BUFFER).contains(&n) {
        return Err(format!("ffi.peek の大きさは 0〜{MAX_BUFFER} です（{n}）"));
    }
    if addr == 0 {
        return Err("ffi.peek のアドレスが NULL です".into());
    }
    // SAFETY: 読めるアドレスかどうかは、呼ぶ側の責任（設計書 5.44）
    let bytes = unsafe { std::slice::from_raw_parts(addr as *const u8, n as usize) };
    Ok(Value::bytes(bytes))
}

fn cstr(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("ffi.cstr", args, 1)?;
    let addr = address_arg("ffi.cstr", args, 0)?;
    // SAFETY: NUL 終端の文字列を指すかどうかは、呼ぶ側の責任（設計書 5.44）
    Ok(unsafe { read_cstr(addr as *const c_void) })
}
