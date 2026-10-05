# Dolphin 処理系の構成

処理系（Rust）の全体像をまとめる。言語仕様は [design.md](design.md)。

状態の書き方: ✅ 実装済み / 🚧 ひな形のみ / ⬜ 未着手

---

## 1. 全体の流れ

```
                ┌──────────────── dolphin（CLI） ────────────────┐
                │   run <file>  /  repl  /  check <file>        │
                └───────────────────────┬────────────────────────┘
                                        ▼
 .dol ─▶ Lexer ─▶ Parser ─▶  AST  ─▶ Resolver ─▶ Interpreter ─▶ 実行結果
        トークン   再帰下降          スコープ検査     木たどり評価
                                    関数の事前登録   （将来 VM に差し替え可）
          │          │                 │               │
          └──────────┴──── Diagnostics ┴───────────────┘
                     行・列つきのエラー表示（ソース行に ^^^）
                                                       │ ネイティブ関数呼び出し
                                                       ▼
        ┌────────────────── 標準ライブラリ ──────────────────┐
        │  io  string  array  math  │  fs  json  │ http │ gfx │
        │        段階 1〜2          │   段階 3   │  4   │  5  │
        └────────────────────────────────────────────────────┘
```

| 層 | 入力 → 出力 | 役割 | 状態 |
|---|---|---|---|
| Lexer | `&str` → `Vec<Token>` | 字句に分割する。`[ ]` `{ }` の中の改行を捨てる | ✅ |
| Parser | `&[Token]` → `Program` | design.md 4 章の EBNF を再帰下降で解析する | 🚧 |
| Resolver | `&Program` → `()` | 関数の事前登録、未定義の関数呼び出し、関数の外での `$x`、引数の数を実行前に検査する | 🚧 |
| Interpreter | `&Program` → 出力 | AST を木たどりで評価する | 🚧 |
| Diagnostics | 各層のエラー → 文字列 | すべての層のエラーを同じ形で表示する | ✅ |
| 標準ライブラリ | `&[Value]` → `Value` | ネイティブ関数を Interpreter に登録する | 🚧 |

Resolver を独立させる理由:

- 「関数の外で `$x` を使った」などの誤りを、実行前にまとめて見つけられる
- 評価器を将来バイトコード VM に差し替えても、前段を書き直さずに済む

---

## 2. CPython との対応

| 役割 | CPython | Dolphin |
|---|---|---|
| 字句解析 | `Parser/tokenizer.c` | `syntax/lexer.rs` |
| 構文解析 | PEG パーサ（`Grammar/python.gram`） | `syntax/parser.rs`（再帰下降） |
| 構文木 | `ast` モジュール | `syntax/ast.rs` |
| 名前解決 | `Python/symtable.c` | `sema/resolver.rs` |
| 実行 | `compile.c` → バイトコード → `ceval.c` | `runtime/interp.rs`（木たどり） |
| 値 | `PyObject` | `runtime/value.rs` の `Value` |
| 組み込み関数 | `builtins`、C 拡張 | `stdlib/`（`NativeFn` を登録） |

---

## 3. ディレクトリ構成

```
dolphin/
├─ Cargo.toml
├─ src/
│  ├─ main.rs            CLI の入口（引数の解析、実行、--tokens でトークン表示）
│  ├─ lib.rs             公開 API（run_source）。CLI・テスト・将来の wasm 版から使う
│  ├─ diag.rs            Diagnostic（エラー表示）
│  ├─ syntax/
│  │  ├─ span.rs         位置情報（行・列）
│  │  ├─ lexer.rs        字句解析
│  │  ├─ ast.rs          Program / Stmt / Expr
│  │  └─ parser.rs       構文解析
│  ├─ sema/
│  │  └─ resolver.rs     実行前の検査
│  ├─ runtime/
│  │  ├─ value.rs        値の型
│  │  ├─ env.rs          Scope（変数表）と Frame（呼び出し 1 回分）
│  │  ├─ error.rs        RuntimeError（呼び出し履歴つき）
│  │  └─ interp.rs       評価器
│  └─ stdlib/
│     ├─ mod.rs          全モジュールの登録、引数検査の共通処理
│     ├─ io.rs           log, input
│     ├─ string.rs       str_len, str_concat
│     ├─ array.rs        arr_len, arr_push
│     ├─ math.rs         ⬜
│     ├─ fs.rs  json.rs  ⬜ 段階 3
│     ├─ http.rs         ⬜ 段階 4（feature = "http"）
│     └─ gfx.rs          ⬜ 段階 5（feature = "gfx"）
├─ tests/
│  ├─ golden.rs          cases/*.dol を実行し、*.out と比較する
│  └─ cases/
├─ examples/             サンプルスクリプト
├─ .github/workflows/    CI（fmt / clippy / test）
└─ docs/
```

- 最初は crate を 1 つにして、モジュールで分ける
- HTTP や GUI の重い依存は cargo の feature で切り替え、使わないときはビルドに含めない
- ビルドが遅くなったら workspace（`crates/dolphin-syntax` など）に分ける。モジュール境界をそのまま crate 境界にできるよう、`syntax` は `runtime` に依存しない

### 依存の向き

```
main.rs ─▶ lib.rs ─┬─▶ syntax ◀─┬─ sema
                   │            │
                   ├─▶ runtime ─┘ （AST を読む）
                   ├─▶ stdlib ──▶ runtime
                   └─▶ diag ◀──── 全層
```

---

## 4. 主な型

### 4.1 AST（`syntax/ast.rs`）

```rust
struct Stmt { kind: StmtKind, span: Span }
enum StmtKind { FuncDef(Rc<FuncDef>), If { .. }, While { .. }, Return(..), Assign { .. }, Expr(..) }

struct Expr { kind: ExprKind, span: Span }
enum ExprKind { Number, Str, Bool, Var, Call, Array, Index, Unary, Binary }
```

- すべての文と式が `Span` を持つ。実行時エラーにも行・列を出すため
- 文字列リテラルは、Lexer の段階で文字の部分と変数の部分（`StrPart`）に分けてある

### 4.2 値（`runtime/value.rs`）

```rust
enum Value {
    Nil,                             // 値なし
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Array(Rc<RefCell<Vec<Value>>>),  // design.md 7.1 の決定次第で変わる
}
```

| 7.1 配列の渡し方 | Rust での実装 | 似ている言語 |
|---|---|---|
| 参照渡し | `Rc<RefCell<Vec>>`（いまの実装） | Python、JavaScript |
| 値渡し | `Rc<Vec>` + `Rc::make_mut`（書き換えるときだけコピーする） | Swift、PHP |

### 4.3 組み込み関数（`runtime/interp.rs`）

```rust
type NativeFn = fn(&mut Interpreter<'_>, &[Value]) -> Result<Value, String>;
```

- エラーはメッセージだけ返し、呼び出し位置は Interpreter が付ける
- `&mut Interpreter` を受け取るので、`log` は `interp.out` に書ける。テストでは `out` に `Vec<u8>` を渡して出力を取り出す

### 4.4 エラー

| 型 | 発生する層 | 中身 |
|---|---|---|
| `Diagnostic` | 全層の共通の形 | メッセージ、位置、補足 |
| `RuntimeError` | Interpreter | メッセージ、位置、呼び出し履歴。`Diagnostic` に変換して表示する |

```
main.dol:2:6: エラー: 文字列が閉じられていません
  |
2 | @b = "open
  |      ^
```

---

## 5. 段階ごとに増えるもの

段階は design.md 2.1 と同じ。

| 段階 | 構文・意味 | ランタイム・標準ライブラリ | ツール・品質 |
|---|---|---|---|
| 1 コア | Parser、AST、Resolver | Value、Interpreter、io / string / array | ゴールデンテスト、CI |
| 2 実用化 | マップ、文字列操作 | string の拡充、map、math | エラー表示の改善、REPL（rustyline） |
| 3 複数ファイル | `import` | モジュールの読み込み、fs、json（serde_json） | `dolphin check` |
| 4 バックエンド | — | http（axum + tokio） | サンプルの API |
| 5 デスクトップ | `gameloop` | gfx（macroquad） | サンプルのゲーム |
| 6 任意 | — | バイトコード VM | ベンチマーク（fib など） |
| おまけ | — | — | VSCode のシンタックスハイライト、wasm で動くブラウザ版 Playground |

---

## 6. 先に分かっている難所

| 箇所 | 問題 | 方針案 |
|---|---|---|
| 段階 4 HTTP | tokio は複数スレッドで動くが、インタプリタは `Rc` を使っていて別スレッドに渡せない（`Send` でない） | サーバーは別スレッドで動かし、リクエストはチャネルでインタプリタのスレッドに送る。インタプリタは 1 スレッドで順番に処理する（JavaScript のイベントループと同じ型） |
| 段階 5 GUI | macroquad がメインループを握る | `gameloop` を「毎フレーム呼ばれる関数」として登録し、macroquad の側から呼ぶ（design.md 7.2） |
| メモリ | `Rc` は循環参照を解放できない（配列が自分自身を含む場合など） | 当面は許容する。問題になったら GC を自作する |
| 性能 | 木たどりは遅い | 段階 6 でバイトコード VM にする。Resolver の層があるので差し替えやすい |

---

## 7. 開発の流れ

| コマンド | 内容 |
|---|---|
| `cargo run -- examples/count.dol` | スクリプトを実行する |
| `cargo run -- --tokens examples/count.dol` | トークン列を表示する |
| `cargo test` | 単体テストを実行する |
| `cargo test -- --ignored` | ゴールデンテストも実行する（Interpreter の完成までは ignore） |
| `cargo fmt` / `cargo clippy` | 整形と静的解析。CI でも同じものを実行する |

テストの置き場所:

- 各層の単体テストは、その層のファイルの `#[cfg(test)] mod tests` に書く
- 言語として正しく動くかは `tests/cases/` に `.dol` と期待出力 `.out` を並べて確かめる。エラーになるケースも、エラー表示を `.out` に書いてテストする
