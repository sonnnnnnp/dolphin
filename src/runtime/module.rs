//! モジュール（design.md 5.7）。import したファイル 1 つ分のトップレベルの変数と関数

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::env::Env;
use super::value::NativeRef;
use crate::diag::Origin;
use crate::syntax::ast::FuncDef;

/// `import` の文字列を、ファイルの場所にする（design.md 5.7・5.38）。
/// `dir` は import するファイルのあるディレクトリ
pub fn resolve_import(dir: &Path, name: &str) -> Result<PathBuf, ImportError> {
    let Some((package, rest)) = split_package(name) else {
        return Ok(dir.join(name));
    };
    // 上の階層の `dolphin_modules/`、続いて環境変数 DOLPHIN_PATH
    let start = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut libs: Vec<PathBuf> = start
        .ancestors()
        .map(|d| d.join("dolphin_modules"))
        .collect();
    if let Some(path) = std::env::var_os("DOLPHIN_PATH") {
        libs.extend(std::env::split_paths(&path).filter(|p| !p.as_os_str().is_empty()));
    }
    for lib in &libs {
        let candidates = match rest {
            None => vec![
                lib.join(package).join("mod.dol"),
                lib.join(format!("{package}.dol")),
            ],
            Some(rest) => vec![lib.join(package).join(format!("{rest}.dol"))],
        };
        if let Some(found) = candidates.into_iter().find(|c| c.is_file()) {
            return Ok(found);
        }
    }
    Err(match rest {
        None => ImportError::NotFound {
            what: format!("パッケージ `{package}`"),
            file: format!("dolphin_modules/{package}/mod.dol` か `dolphin_modules/{package}.dol"),
        },
        Some(rest) => ImportError::NotFound {
            what: format!("モジュール `{package}/{rest}`"),
            file: format!("dolphin_modules/{package}/{rest}.dol"),
        },
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    NotFound { what: String, file: String },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NotFound { what, file } => write!(
                f,
                "{what} が見つかりません（`{file}` を、このファイルのあるディレクトリとその上の階層、環境変数 DOLPHIN_PATH から探しました）"
            ),
        }
    }
}

/// パッケージの指定（`yaylib` `yaylib/client`）なら（パッケージ名, 中のパス）。相対パスなら None
fn split_package(name: &str) -> Option<(&str, Option<&str>)> {
    if name.starts_with("std:") || name.contains(['.', '\\', ':']) || name.starts_with('/') {
        return None;
    }
    let valid = |s: &str| {
        !s.is_empty()
            && !s.starts_with(|c: char| c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match name.split_once('/') {
        None => valid(name).then_some((name, None)),
        Some((package, rest)) => {
            (valid(package) && rest.split('/').all(valid)).then_some((package, Some(rest)))
        }
    }
}

pub struct Module {
    /// 表示用の名前（import したファイルのパス）。実行中の本体は空
    pub name: Rc<str>,
    /// import の相対パスの基準になるディレクトリ
    pub dir: PathBuf,
    /// エラー表示のためのファイルの中身。実行中の本体は None
    pub origin: Option<Rc<Origin>>,
    /// トップレベルのスコープ。ファイル内の関数はここを外側として実行される
    pub globals: Rc<Env>,
    pub functions: RefCell<HashMap<String, Rc<FuncDef>>>,
    /// 組み込みモジュール（`import["std:json"]`）の関数。名前は `parse` のような短い名前
    pub natives: HashMap<&'static str, NativeRef>,
}

impl Module {
    pub fn new(name: Rc<str>, dir: PathBuf, origin: Option<Rc<Origin>>) -> Self {
        Self {
            name,
            dir,
            origin,
            globals: Env::new(None),
            functions: RefCell::default(),
            natives: HashMap::new(),
        }
    }
}

/// 変数が自分自身を指していても無限に表示しないよう、名前だけ出す
impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Module({:?})", self.name)
    }
}
