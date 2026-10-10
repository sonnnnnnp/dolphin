//! モジュール（design.md 5.7）。import したファイル 1 つ分の `@` 変数と関数

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;

use super::env::Scope;
use crate::diag::Origin;
use crate::syntax::ast::FuncDef;

pub struct Module {
    /// 表示用の名前（import したファイルのパス）。実行中の本体は空
    pub name: Rc<str>,
    /// import の相対パスの基準になるディレクトリ
    pub dir: PathBuf,
    /// エラー表示のためのファイルの中身。実行中の本体は None
    pub origin: Option<Rc<Origin>>,
    pub globals: RefCell<Scope>,
    pub functions: RefCell<HashMap<String, Rc<FuncDef>>>,
}

impl Module {
    pub fn new(name: Rc<str>, dir: PathBuf, origin: Option<Rc<Origin>>) -> Self {
        Self {
            name,
            dir,
            origin,
            globals: RefCell::default(),
            functions: RefCell::default(),
        }
    }
}

/// 変数が自分自身を指していても無限に表示しないよう、名前だけ出す
impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Module({:?})", self.name)
    }
}
