//! Dolphin 処理系の本体。CLI・テスト・将来の wasm 版から使う
//!
//! ソース → Lexer → Parser → AST → Resolver → Interpreter
//! 全体像は docs/architecture.md

pub mod diag;
pub mod runtime;
pub mod sema;
pub mod stdlib;
pub mod syntax;

use std::io::Write;
use std::path::PathBuf;

use diag::Diagnostic;
use runtime::interp::Interpreter;
use runtime::value::Value;

/// ソースを解析して実行する。`log` の出力は `out` に書く
pub fn run_source(src: &str, out: &mut dyn Write) -> Result<(), Diagnostic> {
    run_source_in(src, PathBuf::from("."), out)
}

/// `import` の相対パスを `base_dir` から引いて実行する（普通は実行するファイルのあるディレクトリ）
pub fn run_source_in(src: &str, base_dir: PathBuf, out: &mut dyn Write) -> Result<(), Diagnostic> {
    Session::with_base_dir(out, base_dir).eval(src)?;
    Ok(())
}

/// 変数と関数を保ったまま、ソースを何回かに分けて実行する（REPL で使う）
pub struct Session<'a> {
    interp: Interpreter<'a>,
}

impl<'a> Session<'a> {
    /// `import` の相対パスはカレントディレクトリから引く
    pub fn new(out: &'a mut dyn Write) -> Self {
        Self::with_base_dir(out, PathBuf::from("."))
    }

    pub fn with_base_dir(out: &'a mut dyn Write, base_dir: PathBuf) -> Self {
        let mut interp = Interpreter::new(out, base_dir);
        stdlib::register(&mut interp);
        Self { interp }
    }

    /// `src` を実行する。最後の文が式なら、その値を返す
    pub fn eval(&mut self, src: &str) -> Result<Option<Value>, Diagnostic> {
        let tokens = syntax::lexer::tokenize(src)?;
        let program = syntax::parser::parse(&tokens)?;
        sema::resolver::resolve(&program, |name| self.interp.callee(name))?;
        Ok(self.interp.run(&program)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keeps_state() {
        let mut out = Vec::new();
        let mut session = Session::new(&mut out);
        session.eval("@x = 1").unwrap();
        session.eval("inc[$n] (\n    # $n + 1\n)").unwrap();
        let value = session.eval("inc[@x]").unwrap();
        assert!(matches!(value, Some(Value::Num(2.0))));

        // 前の入力で定義した関数は再定義でき、引数の数も検査される
        session.eval("inc[$n] (\n    # $n + 10\n)").unwrap();
        assert!(matches!(
            session.eval("inc[1]").unwrap(),
            Some(Value::Num(11.0))
        ));
        let e = session.eval("inc[1, 2]").unwrap_err();
        assert_eq!(e.message, "`inc` の引数は 1 個です（2 個渡されました）");
    }

    #[test]
    fn statement_has_no_value() {
        let mut out = Vec::new();
        let mut session = Session::new(&mut out);
        assert!(session.eval("@x = 1").unwrap().is_none());
        assert!(session.eval("").unwrap().is_none());
    }

    #[test]
    fn incomplete_input_is_marked() {
        let mut out = Vec::new();
        let mut session = Session::new(&mut out);
        for src in [
            "f[] (",
            "@m = {\"a\": 1,",
            "log[\"abc",
            "if true (\n    log[1]\n",
        ] {
            let e = session.eval(src).unwrap_err();
            assert!(e.unexpected_eof, "{src:?}: {}", e.message);
        }
        for src in ["@x = )", "log[1 2]"] {
            let e = session.eval(src).unwrap_err();
            assert!(!e.unexpected_eof, "{src:?}: {}", e.message);
        }
    }
}
