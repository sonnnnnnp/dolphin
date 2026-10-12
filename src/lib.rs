//! Dolphin 処理系の本体。CLI・テスト・将来の wasm 版から使う
//!
//! ソース → Lexer → Parser → AST → Resolver → Interpreter
//! 全体像は docs/architecture.md

pub mod diag;
pub mod fmt;
pub mod runtime;
pub mod sema;
pub mod stdlib;
pub mod syntax;
pub mod testrun;

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
    Session::with_base_dir(out, base_dir).run(src)
}

/// コマンドライン引数つきでスクリプトを実行し、終了コードを返す。`os.exit` で終われば
/// その終了コード、最後まで実行できれば 0
pub fn run_script(
    src: &str,
    base_dir: PathBuf,
    args: Vec<String>,
    out: &mut dyn Write,
) -> Result<i32, Diagnostic> {
    let mut session = Session::with_base_dir(out, base_dir);
    session.interp.args = args;
    session.run(src)?;
    Ok(session.exit_code().unwrap_or(0))
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

    /// `os.exit` が呼ばれていたら、その終了コード
    pub fn exit_code(&self) -> Option<i32> {
        self.interp.exit_code
    }

    /// `src` を実行する。最後の文が式なら、その値を返す（REPL で表示するため）
    pub fn eval(&mut self, src: &str) -> Result<Option<Value>, Diagnostic> {
        self.exec(src, true)
    }

    /// 実行せずに、構文と Resolver の検査だけを行う（`dolphin check`）
    pub fn check(&mut self, src: &str) -> Result<(), Diagnostic> {
        let tokens = syntax::lexer::tokenize(src)?;
        let program = syntax::parser::parse(&tokens)?;
        sema::resolver::resolve(
            &program,
            |name| self.interp.callee(name),
            |name| self.interp.has_global(name),
        )
    }

    /// 名前つきの関数を定義の順に（名前、引数の数）で返す（`dolphin test`）
    pub fn user_functions(&self) -> Vec<(String, usize)> {
        self.interp.user_functions()
    }

    /// `src` を実行する。値は返さない（ファイルの実行）
    pub fn run(&mut self, src: &str) -> Result<(), Diagnostic> {
        self.exec(src, false)?;
        // 待っているもの（タイマー・非同期の関数）がある間は、イベントループを動かす（design.md 5.23）。
        // Err は `os.exit`（終了コードは `exit_code` にある）
        if self.interp.exit_code.is_none() && self.interp.events.has_pending() {
            let _ = self.interp.run_event_loop();
        }
        Ok(())
    }

    fn exec(&mut self, src: &str, keep_last: bool) -> Result<Option<Value>, Diagnostic> {
        let tokens = syntax::lexer::tokenize(src)?;
        let program = syntax::parser::parse(&tokens)?;
        sema::resolver::resolve(
            &program,
            |name| self.interp.callee(name),
            |name| self.interp.has_global(name),
        )?;
        match self.interp.run(&program, keep_last) {
            // `os.exit` はエラーではなく、静かに実行を止める
            Err(e) if e.exit => Ok(None),
            result => Ok(result?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keeps_state() {
        let mut out = Vec::new();
        let mut session = Session::new(&mut out);
        session.eval("@x := 1").unwrap();
        session.eval("inc[@n] (\n    # @n + 1\n)").unwrap();
        let value = session.eval("inc[@x]").unwrap();
        assert!(matches!(value, Some(Value::Num(2.0))));

        // 前の入力で定義した関数は再定義でき、引数の数も検査される
        session.eval("inc[@n] (\n    # @n + 10\n)").unwrap();
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
        assert!(session.eval("@x := 1").unwrap().is_none());
        assert!(session.eval("").unwrap().is_none());
    }

    #[test]
    fn incomplete_input_is_marked() {
        let mut out = Vec::new();
        let mut session = Session::new(&mut out);
        for src in [
            "f[] (",
            "@m := {\"a\": 1,",
            "log[\"abc",
            "if true (\n    log[1]\n",
        ] {
            let e = session.eval(src).unwrap_err();
            assert!(e.unexpected_eof, "{src:?}: {}", e.message);
        }
        for src in ["@x := )", "log[1 2]"] {
            let e = session.eval(src).unwrap_err();
            assert!(!e.unexpected_eof, "{src:?}: {}", e.message);
        }
    }
}
