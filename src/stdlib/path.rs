//! `std:path`: join, dirname, basename, stem, ext, normalize, is_absolute, absolute, sep
//! （design.md 5.13）
//!
//! ファイルには触れず、文字列として組み立てる・分解する。入力はそのシステムの区切りを受け付け、
//! 出力は常に `/` 区切りにする（Windows も `/` を受け付けるので、スクリプトがシステムによらず
//! 同じ結果になる）

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use super::{error_value, expect_args, str_arg};
use crate::runtime::interp::{Const, Interpreter};
use crate::runtime::value::Value;

pub fn register(interp: &mut Interpreter<'_>) {
    interp.register_module_constants("std:path", &[("sep", Const::Str("/"))]);
    interp.register_module(
        "std:path",
        &[
            ("path.join", join),
            ("path.dirname", dirname),
            ("path.basename", basename),
            ("path.stem", stem),
            ("path.ext", ext),
            ("path.normalize", normalize),
            ("path.is_absolute", is_absolute),
            ("path.absolute", absolute),
        ],
    );
}

/// `/` 区切りの文字列にする。Windows 以外では、`\` は名前の一部なのでそのまま
pub(super) fn to_slash(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

fn str_value(s: impl Into<String>) -> Value {
    Value::Str(s.into().into())
}

/// つなげる。あとの部分が絶対パスなら、それ以前を捨てる
fn join(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("path.join の引数は 1 個以上です（0 個渡されました）".into());
    }
    let mut path = PathBuf::new();
    for i in 0..args.len() {
        path.push(str_arg("path.join", args, i)?);
    }
    Ok(str_value(to_slash(&path)))
}

/// ディレクトリ部分。`dirname["a"]` は `""`、ルートはルート自身
fn dirname(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.dirname", args, 1)?;
    let path = Path::new(str_arg("path.dirname", args, 0)?);
    let dir = match path.parent() {
        Some(parent) => to_slash(parent),
        // 親がないのは、ルートか空のパス
        None if path.has_root() => to_slash(path),
        None => String::new(),
    };
    Ok(str_value(dir))
}

/// 最後の名前。ルートは `""`、`a/..` は `".."`
fn basename(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.basename", args, 1)?;
    Ok(str_value(last_name(str_arg("path.basename", args, 0)?)))
}

fn last_name(path: &str) -> String {
    match Path::new(path).components().next_back() {
        Some(Component::Normal(name)) => name.to_string_lossy().into_owned(),
        Some(Component::ParentDir) => "..".to_string(),
        Some(Component::CurDir) => ".".to_string(),
        _ => String::new(),
    }
}

/// 拡張子を除いた最後の名前
fn stem(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.stem", args, 1)?;
    let name = last_name(str_arg("path.stem", args, 0)?);
    let stem = Path::new(&name).file_stem().map(OsStr::to_string_lossy);
    Ok(str_value(stem.as_deref().unwrap_or("")))
}

/// 拡張子（`"."` を含む）。なければ `""`
fn ext(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.ext", args, 1)?;
    let name = last_name(str_arg("path.ext", args, 0)?);
    let ext = Path::new(&name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    Ok(str_value(ext.unwrap_or_default()))
}

/// `.` と `..` を字面だけで整理する。空になったら `"."`
fn normalize_path(path: &Path) -> PathBuf {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match parts.last() {
                Some(Component::Normal(_)) => {
                    parts.pop();
                }
                // ルートより上へは行けない
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => parts.push(component),
            },
            other => parts.push(other),
        }
    }
    let result: PathBuf = parts.iter().collect();
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

fn normalize(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.normalize", args, 1)?;
    let path = Path::new(str_arg("path.normalize", args, 0)?);
    Ok(str_value(to_slash(&normalize_path(path))))
}

/// ルートから始まるか（Windows の `/a` や `C:\a` も絶対パスとみなす）
fn is_absolute(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.is_absolute", args, 1)?;
    let path = Path::new(str_arg("path.is_absolute", args, 0)?);
    Ok(Value::Bool(path.has_root()))
}

/// カレントディレクトリを基準に絶対パスにして、字面だけで整理する
fn absolute(_: &mut Interpreter<'_>, args: &[Value]) -> Result<Value, String> {
    expect_args("path.absolute", args, 1)?;
    let path = str_arg("path.absolute", args, 0)?;
    match std::path::absolute(path) {
        Ok(abs) => Ok(str_value(to_slash(&normalize_path(&abs)))),
        Err(e) => {
            let (kind, reason) = super::fs::classify(&e);
            Ok(error_value(
                kind,
                format!("`{path}` を絶対パスにできません: {reason}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> String {
        to_slash(&normalize_path(Path::new(s)))
    }

    #[test]
    fn normalize_cases() {
        assert_eq!(norm("a/./b//c"), "a/b/c");
        assert_eq!(norm("a/b/../c"), "a/c");
        assert_eq!(norm("a/.."), ".");
        assert_eq!(norm(""), ".");
        assert_eq!(norm("../a"), "../a");
        assert_eq!(norm("a/../.."), "..");
        assert_eq!(norm("./"), ".");
    }

    #[test]
    fn normalize_stays_below_root() {
        assert_eq!(norm("/a/../.."), "/");
        assert_eq!(norm("/.."), "/");
    }

    #[test]
    fn last_names() {
        assert_eq!(last_name("a/b/"), "b");
        assert_eq!(last_name("a"), "a");
        assert_eq!(last_name("a/.."), "..");
        assert_eq!(last_name("/"), "");
        assert_eq!(last_name(""), "");
    }
}
