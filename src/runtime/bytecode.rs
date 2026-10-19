//! バイトコード（design.md 5.45）。関数・ファイルの本体を、命令の列にコンパイルしたもの。
//! 命令は `compile.rs` が作り、`interp/vm.rs` の VM が実行する

use std::rc::Rc;

use super::value::Value;
use crate::syntax::ast::{BinaryOp, FuncDef, Pattern, UnaryOp};
use crate::syntax::span::Span;

/// 命令。数値は `Chunk` の表への番号、`u32` の行き先は命令の位置
#[derive(Debug, Clone)]
pub enum Op {
    // ---- 値・変数・スタック ----
    /// 定数（`consts`）をスタックに積む
    Const(u32),
    /// 変数を読んで積む。なければ「未定義の変数」
    LoadVar(u32),
    /// スタックの値で、いまのスコープに新しい変数を作る
    Declare(u32),
    /// スタックの値を、宣言済みの変数に代入する。なければ「未定義の変数」
    AssignVar(u32),
    Pop,
    /// 式文の値を捨てる。エラー値なら、捨てたことを実行時エラーにする
    PopCheckError,
    PushScope,
    PopScope,

    // ---- ジャンプ ----
    Jump(u32),
    /// 条件（真偽値）を取り出して、偽なら飛ぶ。真偽値でなければエラー
    JumpIfFalseCond(u32),
    /// ループの繰り返しごと（循環回収器を動かす機会）
    LoopTick,

    // ---- 演算 ----
    /// `&&` `||` の片側の値が真偽値か確かめる（積んだまま）
    LogicOperand(BinaryOp),
    /// `&&` は偽、`||` は真なら、値を残して飛ぶ。そうでなければ値を捨てて進む
    ShortCircuit(BinaryOp, u32),
    Binary(BinaryOp),
    Unary(UnaryOp),

    // ---- 配列・マップ・文字列 ----
    /// スタックの上から n 個で配列を作る
    MakeArray(u32),
    /// 空の配列を積む（展開を含む呼び出し・配列リテラルを組み立てる）
    ArrayNew,
    /// 配列（スタックの 2 番目）に値（一番上）を足す
    ArrayAppend,
    /// 配列（スタックの 2 番目）に、配列（一番上）の要素を足す。配列でなければエラー
    ArrayExtend,
    /// マップのキーが文字列か確かめて、積んだまま
    CheckKey,
    /// スタックの上から 2n 個（キー、値の順）でマップを作る
    MakeMap(u32),
    /// 値を `log` と同じ表示の文字列にする
    ToDisplay,
    /// スタックの上から n 個の文字列をつなげる
    Concat(u32),

    // ---- 添字・メンバー ----
    /// `container{key}` を読む。スタックは [container, key]
    IndexGet {
        target: Span,
        key: Span,
    },
    /// `container{key} = value`。スタックは [container, key, value]
    IndexSet {
        target: Span,
        key: Span,
    },
    /// 一番上のコンテナがモジュールなら、書き換えられないエラー
    RejectModule,
    /// `@u.name`。スタックは [target]
    Member {
        name: u32,
        target: Span,
    },

    // ---- 関数 ----
    /// 関数名だけを書いたときの関数値
    FuncRef(u32),
    MakeClosure(u32),
    /// 値がエラー値なら、いまの関数からそのエラーを返す
    Try,
    Return,
    /// `name[args]`。スタックは [args...]
    Call {
        name: u32,
        argc: u32,
    },
    /// 引数が展開を含むとき。スタックは [引数の配列]
    CallSpread {
        name: u32,
    },
    /// `callee[args]`。スタックは [callee, args...]
    CallValue {
        argc: u32,
        callee: Span,
    },
    CallValueSpread {
        callee: Span,
    },
    /// `@u.name[args]`。スタックは [target, args...]
    MemberCall {
        name: u32,
        argc: u32,
        target: Span,
    },
    MemberCallSpread {
        name: u32,
        target: Span,
    },
    /// 既定値を評価する前に、その引数が渡されていれば飛ぶ
    SkipIfGiven {
        index: u32,
        target: u32,
    },

    // ---- 分割代入・for ----
    Destructure {
        pattern: u32,
        declare: bool,
    },
    /// 配列を取り出し、要素の写しと位置 0 を積む（`for` の準備）
    ForPrep,
    /// 次の要素をスコープの変数に入れる。なくなったら `end` へ飛ぶ
    ForNext {
        end: u32,
        var: u32,
    },
}

/// コンパイルした 1 つのまとまり（関数 1 つ、またはファイルの本体）
#[derive(Debug)]
pub struct Chunk {
    pub code: Vec<Op>,
    /// 命令ごとの位置（エラー表示用）。`code` と同じ長さ
    pub spans: Vec<Span>,
    pub consts: Vec<Value>,
    pub names: Vec<String>,
    /// `MakeClosure` が作る無名関数の定義
    pub defs: Vec<Rc<FuncDef>>,
    pub patterns: Vec<Pattern>,
}
