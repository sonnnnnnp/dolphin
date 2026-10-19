# 🐬 Dolphin

`@` で変数を宣言する、Rust 製の小さなインタプリタ言語。拡張子は `.dol`。

## このプロジェクトについて

インタプリタ言語がどう動いているのかを理解するために、インタプリタ言語を自作しています。せっかく自作するので、シンタックスはユニークにしました。

作る理由は、学習とロマンと自己満足です。実用を目的にはしていませんが、ロマンとして実際に使っても困らない水準を目標にしています。バックエンドの API やデスクトップアプリも作れる言語を目指しています。

```
@name := "Dolphin"
log["Hello, @name!"]

add[@a, @b] (
    # @a + @b
)

@i := 1
while @i <= 3 (
    log["count: @i"]
    @i = add[@i, 1]
)

@user := {"name": @name, "langs": {"Rust", "C++"}}
log[@user]
```

実行結果:

```
Hello, Dolphin!
count: 1
count: 2
count: 3
{"name": "Dolphin", "langs": {"Rust", "C++"}}
```

## なぜ `@` なのか

<!-- ここに書く -->

## 構文の早見表

| 書き方 | 意味 |
|---|---|
| `@x := 1` | 変数の宣言。いま入っているスコープ（ファイル・関数・`( )` のブロック）に作る |
| `@x = 2` | 宣言済みの変数への代入（宣言していなければ実行前エラー） |
| `name[a, b]` | 関数呼び出し |
| `name[@a, @b] ( … )` | 関数定義 |
| `[@a] ( … )` / `@f := name` / `@f[1]` | 無名関数、関数値、関数値の呼び出し。外側の変数を捕まえる（クロージャ） |
| `# value` | 関数から値を返す |
| `@x := f[…]?` | `f` が返したエラー値は、いまの関数からそのまま返す。`is_error[@r]` で見分ける |
| `if 条件 ( … )` / `else ( … )` / `else if` | 条件分岐 |
| `while 条件 ( … )` / `for @x in 配列 ( … )` / `break` / `continue` | 繰り返し |
| `nil` | 値なし |
| `{1, 2, 3}` / `@a{0}` | 配列。添字は `{ }` |
| `{"key": value}` / `@m{"key"}` / `{:}` | マップ（`{:}` は空のマップ） |
| `"Hi, @name"` | 文字列。中の変数は展開される |
| `// コメント` | 行末までコメント |

詳しい仕様は [docs/design.md](docs/design.md)、組み込み関数の一覧は [design.md 5.5](docs/design.md#55-組み込み関数) にあります。

## できること

段階は [docs/design.md](docs/design.md) の 2.1 に対応しています。

- [x] **段階 1: コア言語**
  - 変数（`@`。`:=` で宣言、`=` で代入、`( )` ごとのスコープ）、四則演算・剰余・比較・論理演算（短絡評価）、単項 `-` `!`、`( )` によるグループ化
  - `if` / `else` / `else if`、`while`、`break` / `continue`
  - 関数（定義より前から呼べる、再帰、`#` で戻り値）
  - 配列、文字列の中の `@x` の展開、行末コメント
- [x] **段階 2: 実用化**
  - 行・列と呼び出し履歴つきのエラー表示
  - マップ（連想配列）
  - 文字列・数値変換・数学の組み込み関数
  - 対話モード（REPL）
- [x] **段階 3.5**: 言語の土台の見直し（[design.md 8 章](docs/design.md)）
  - [x] 変数を `@` に統一、`:=` で宣言、添字を `{ }` に、スコープ
  - [x] 関数値とクロージャ（`[@n] ( # @n + 1 )`、`arr_map` など）
  - [x] `nil`、`for`、`range`
  - [x] エラー処理（エラー値と `?`）
  - [ ] 実行モデル（イベントループ）
- [ ] **段階 3**: 複数ファイル、ファイル入出力
  - [x] `import`（名前空間つき。`@u := import["util.dol"]`、`@u.add[1, 2]`）
  - [x] ファイル入出力（`file_read` `file_write` `file_append` `file_exists`）
- [ ] **段階 4**: HTTP、JSON → バックエンド API
  - [x] 組み込みモジュール（`import["std:..."]`）と JSON（`std:json`）
  - [x] HTTP クライアント（`std:http`）
  - [ ] HTTP サーバー
- [ ] **段階 5**: ウィンドウ・描画・入力 → デスクトップアプリ（旧 C++ 実装では SFML で対応済み）

## 作ってみたもの

`examples/` に、Dolphin で書いたプログラムがあります。

| ファイル | 内容 |
|---|---|
| [dolvc.dol](examples/dolvc.dol) | 小さなバージョン管理ツール（git のミニ版）。`commit` `status` `diff` `log` `checkout`。ファイルの中身は sha256 で名前づけして保存する |
| [dolvc_demo.dol](examples/dolvc_demo.dol) | `dolvc` を一時ディレクトリで通して動かすデモ（`cargo run -- examples/dolvc_demo.dol`） |
| [life.dol](examples/life.dol) | ライフゲームのアニメーション（グライダー銃） |
| [mandelbrot.dol](examples/mandelbrot.dol) | マンデルブロ集合を文字で描く |
| [json.dol](examples/json.dol) / [http.dol](examples/http.dol) | JSON の読み書き / HTTP で JSON を取ってくる（ネットワークが必要） |
| [closure.dol](examples/closure.dol) | クロージャと `arr_map` |

## コードを段階ごとに読む

機能ごとにタグを付けています。何が増えたかは [CHANGELOG.md](CHANGELOG.md) にまとめてあり、タグの間の差分を見ると、そのコードを読めます。

| タグ | 内容 | 差分（前のタグから） |
|---|---|---|
| [v0.1.0](https://github.com/sonnnnnnp/dolphin/tree/v0.1.0) | 段階 1: コア言語 | — |
| [v0.2.0](https://github.com/sonnnnnnp/dolphin/tree/v0.2.0) | 段階 2: エラー表示、文字列、マップ、REPL | [v0.1.0...v0.2.0](https://github.com/sonnnnnnp/dolphin/compare/v0.1.0...v0.2.0) |
| [v0.3.0](https://github.com/sonnnnnnp/dolphin/tree/v0.3.0) | `import`（名前空間つき） | [v0.2.0...v0.3.0](https://github.com/sonnnnnnp/dolphin/compare/v0.2.0...v0.3.0) |
| [v0.4.0](https://github.com/sonnnnnnp/dolphin/tree/v0.4.0) | ファイル入出力 | [v0.3.0...v0.4.0](https://github.com/sonnnnnnp/dolphin/compare/v0.3.0...v0.4.0) |
| [v0.5.0](https://github.com/sonnnnnnp/dolphin/tree/v0.5.0) | 文法の見直し（`$` の廃止、`:=`、添字 `{ }`、スコープ） | [v0.4.0...v0.5.0](https://github.com/sonnnnnnp/dolphin/compare/v0.4.0...v0.5.0) |
| [v0.6.0](https://github.com/sonnnnnnp/dolphin/tree/v0.6.0) | 関数値とクロージャ | [v0.5.0...v0.6.0](https://github.com/sonnnnnnp/dolphin/compare/v0.5.0...v0.6.0) |
| [v0.7.0](https://github.com/sonnnnnnp/dolphin/tree/v0.7.0) | `nil`、`for`、`range` | [v0.6.0...v0.7.0](https://github.com/sonnnnnnp/dolphin/compare/v0.6.0...v0.7.0) |
| [v0.8.0](https://github.com/sonnnnnnp/dolphin/tree/v0.8.0) | エラー値と `?` | [v0.7.0...v0.8.0](https://github.com/sonnnnnnp/dolphin/compare/v0.7.0...v0.8.0) |
| [v0.9.0](https://github.com/sonnnnnnp/dolphin/tree/v0.9.0) | 組み込みモジュールと `std:json` | [v0.8.0...v0.9.0](https://github.com/sonnnnnnp/dolphin/compare/v0.8.0...v0.9.0) |
| [v0.10.0](https://github.com/sonnnnnnp/dolphin/tree/v0.10.0) | HTTP クライアント `std:http` | [v0.9.0...v0.10.0](https://github.com/sonnnnnnp/dolphin/compare/v0.9.0...v0.10.0) |
| [v0.11.0](https://github.com/sonnnnnnp/dolphin/tree/v0.11.0) | 基本型の関数（配列・マップ・文字列・数値）、`type_of`、`std:math` | [v0.10.0...v0.11.0](https://github.com/sonnnnnnp/dolphin/compare/v0.10.0...v0.11.0) |
| [v0.12.0](https://github.com/sonnnnnnp/dolphin/tree/v0.12.0) | `std:time`、`std:os`、`std:path`、ファイルの残り | [v0.11.0...v0.12.0](https://github.com/sonnnnnnp/dolphin/compare/v0.11.0...v0.12.0) |
| [v0.13.0](https://github.com/sonnnnnnp/dolphin/tree/v0.13.0) | バイト列、`std:encoding`、`std:crypto` | [v0.12.0...v0.13.0](https://github.com/sonnnnnnp/dolphin/compare/v0.12.0...v0.13.0) |
| [v0.14.0](https://github.com/sonnnnnnp/dolphin/tree/v0.14.0) | 例（`dolvc`、ライフゲーム、マンデルブロ）、`os.exe` | [v0.13.0...v0.14.0](https://github.com/sonnnnnnp/dolphin/compare/v0.13.0...v0.14.0) |
| [v0.15.0](https://github.com/sonnnnnnp/dolphin/tree/v0.15.0) | 循環回収器、`gc_collect` | [v0.14.0...v0.15.0](https://github.com/sonnnnnnp/dolphin/compare/v0.14.0...v0.15.0) |
| [v0.16.0](https://github.com/sonnnnnnp/dolphin/tree/v0.16.0) | HTTP サーバー `http.serve` | [v0.15.0...v0.16.0](https://github.com/sonnnnnnp/dolphin/compare/v0.15.0...v0.16.0) |
| [v0.17.0](https://github.com/sonnnnnnp/dolphin/tree/v0.17.0) | `assert` `assert_eq` `dbg`、`dolphin check`、`dolphin test` | [v0.16.0...v0.17.0](https://github.com/sonnnnnnp/dolphin/compare/v0.16.0...v0.17.0) |
| [v0.18.0](https://github.com/sonnnnnnp/dolphin/tree/v0.18.0) | `dolphin fmt` | [v0.17.0...v0.18.0](https://github.com/sonnnnnnp/dolphin/compare/v0.17.0...v0.18.0) |
| [v0.19.0](https://github.com/sonnnnnnp/dolphin/tree/v0.19.0) | 数値リテラル（`0xFF` `1_000` `1e9`） | [v0.18.0...v0.19.0](https://github.com/sonnnnnnp/dolphin/compare/v0.18.0...v0.19.0) |
| [v0.20.0](https://github.com/sonnnnnnp/dolphin/tree/v0.20.0) | 組み込み関数を隠す定義 | [v0.19.0...v0.20.0](https://github.com/sonnnnnnp/dolphin/compare/v0.19.0...v0.20.0) |
| [v0.21.0](https://github.com/sonnnnnnp/dolphin/tree/v0.21.0) | `@m.key` | [v0.20.0...v0.21.0](https://github.com/sonnnnnnp/dolphin/compare/v0.20.0...v0.21.0) |
| [v0.22.0](https://github.com/sonnnnnnp/dolphin/tree/v0.22.0) | 条件式（`if c ( a ) else ( b )`） | [v0.21.0...v0.22.0](https://github.com/sonnnnnnp/dolphin/compare/v0.21.0...v0.22.0) |
| [v0.23.0](https://github.com/sonnnnnnp/dolphin/tree/v0.23.0) | 引数の既定値 | [v0.22.0...v0.23.0](https://github.com/sonnnnnnp/dolphin/compare/v0.22.0...v0.23.0) |
| [v0.24.0](https://github.com/sonnnnnnp/dolphin/tree/v0.24.0) | 可変長引数 `...@rest` と展開 `...式` | [v0.23.0...v0.24.0](https://github.com/sonnnnnnp/dolphin/compare/v0.23.0...v0.24.0) |
| [v0.25.0](https://github.com/sonnnnnnp/dolphin/tree/v0.25.0) | 文字列の式展開 `"@{a + b}"` と生の文字列 `r"…"` | [v0.24.0...v0.25.0](https://github.com/sonnnnnnp/dolphin/compare/v0.24.0...v0.25.0) |
| [v0.26.0](https://github.com/sonnnnnnp/dolphin/tree/v0.26.0) | 分割代入（`{@a, @b} := …`） | [v0.25.0...v0.26.0](https://github.com/sonnnnnnp/dolphin/compare/v0.25.0...v0.26.0) |
| [v0.27.0](https://github.com/sonnnnnnp/dolphin/tree/v0.27.0) | ビット演算 `std:bits` | [v0.26.0...v0.27.0](https://github.com/sonnnnnnp/dolphin/compare/v0.26.0...v0.27.0) |
| [v0.28.0](https://github.com/sonnnnnnp/dolphin/tree/v0.28.0) | `std:regex`（正規表現） | [v0.27.0...v0.28.0](https://github.com/sonnnnnnp/dolphin/compare/v0.27.0...v0.28.0) |
| [v0.29.0](https://github.com/sonnnnnnp/dolphin/tree/v0.29.0) | TCP `std:net` と `handle` 型 | [v0.28.0...v0.29.0](https://github.com/sonnnnnnp/dolphin/compare/v0.28.0...v0.29.0) |
| [v0.30.0](https://github.com/sonnnnnnp/dolphin/tree/v0.30.0) | WebSocket（`http.serve` の `websocket` と `std:ws`） | [v0.29.0...v0.30.0](https://github.com/sonnnnnnp/dolphin/compare/v0.29.0...v0.30.0) |
| [v0.31.0](https://github.com/sonnnnnnp/dolphin/tree/v0.31.0) | 非同期の関数（`std:async`、`http.get_async` など） | [v0.30.0...v0.31.0](https://github.com/sonnnnnnp/dolphin/compare/v0.30.0...v0.31.0) |
| [v0.32.0](https://github.com/sonnnnnnp/dolphin/tree/v0.32.0) | `std:ui`（ウィンドウと宣言的 UI。`--features ui`） | [v0.31.0...v0.32.0](https://github.com/sonnnnnnp/dolphin/compare/v0.31.0...v0.32.0) |
| [v0.33.0](https://github.com/sonnnnnnp/dolphin/tree/v0.33.0) | `std:canvas`（ゲーム・描画。`--features ui`） | [v0.32.0...v0.33.0](https://github.com/sonnnnnnp/dolphin/compare/v0.32.0...v0.33.0) |
| [v0.34.0](https://github.com/sonnnnnnp/dolphin/tree/v0.34.0) | 画像（`canvas.image`） | [v0.33.0...v0.34.0](https://github.com/sonnnnnnp/dolphin/compare/v0.33.0...v0.34.0) |
| [v0.35.0](https://github.com/sonnnnnnp/dolphin/tree/v0.35.0) | 音 `std:audio`（`--features audio`） | [v0.34.0...v0.35.0](https://github.com/sonnnnnnp/dolphin/compare/v0.34.0...v0.35.0) |
| [v0.36.0](https://github.com/sonnnnnnp/dolphin/tree/v0.36.0) | SQLite `std:sqlite`（`--features sqlite`） | [v0.35.0...v0.36.0](https://github.com/sonnnnnnp/dolphin/compare/v0.35.0...v0.36.0) |
| [v0.37.0](https://github.com/sonnnnnnp/dolphin/tree/v0.37.0) | CSV `std:csv` | [v0.36.0...v0.37.0](https://github.com/sonnnnnnp/dolphin/compare/v0.36.0...v0.37.0) |
| [v0.38.0](https://github.com/sonnnnnnp/dolphin/tree/v0.38.0) | 日付の計算（`std:time` の `add_months` `start_of` など） | [v0.37.0...v0.38.0](https://github.com/sonnnnnnp/dolphin/compare/v0.37.0...v0.38.0) |
| [v0.39.0](https://github.com/sonnnnnnp/dolphin/tree/v0.39.0) | 乱数 `std:random`（`seed` `choice` `shuffle` `sample` など） | [v0.38.0...v0.39.0](https://github.com/sonnnnnnp/dolphin/compare/v0.38.0...v0.39.0) |
| [v0.40.0](https://github.com/sonnnnnnp/dolphin/tree/v0.40.0) | `std:collections`（ヒープ・`counts` `unique` `group_by` `chunk` `zip` `flatten`） | [v0.39.0...v0.40.0](https://github.com/sonnnnnnp/dolphin/compare/v0.39.0...v0.40.0) |
| [v0.41.0](https://github.com/sonnnnnnp/dolphin/tree/v0.41.0) | 言語サーバー `dolphin lsp`（定義へ移動・エラー表示・ホバー・アウトライン・整形）と VS Code 拡張 | [v0.40.0...v0.41.0](https://github.com/sonnnnnnp/dolphin/compare/v0.40.0...v0.41.0) |
| [v0.42.0](https://github.com/sonnnnnnp/dolphin/tree/v0.42.0) | 言語サーバーに補完・参照の検索・リネーム | [v0.41.0...v0.42.0](https://github.com/sonnnnnnp/dolphin/compare/v0.41.0...v0.42.0) |
| [v0.43.0](https://github.com/sonnnnnnp/dolphin/tree/v0.43.0) | `wss://`（WebSocket の TLS） | [v0.42.0...v0.43.0](https://github.com/sonnnnnnp/dolphin/compare/v0.42.0...v0.43.0) |
| [v0.44.0](https://github.com/sonnnnnnp/dolphin/tree/v0.44.0) | 画像の加工 `std:image`（`info` `resize` `crop` `convert`） | [v0.43.0...v0.44.0](https://github.com/sonnnnnnp/dolphin/compare/v0.43.0...v0.44.0) |
| [v0.45.0](https://github.com/sonnnnnnp/dolphin/tree/v0.45.0) | WebSocket のコールバック版 `ws.connect_async` | [v0.44.0...v0.45.0](https://github.com/sonnnnnnp/dolphin/compare/v0.44.0...v0.45.0) |
| [v0.46.0](https://github.com/sonnnnnnp/dolphin/tree/v0.46.0) | パッケージの `import["名前"]`（`dolphin_modules/` と `DOLPHIN_PATH`） | [v0.45.0...v0.46.0](https://github.com/sonnnnnnp/dolphin/compare/v0.45.0...v0.46.0) |
| [v0.47.0](https://github.com/sonnnnnnp/dolphin/tree/v0.47.0) | エラー値の詳細データ（`err_new` の第 3 引数、`err_data`） | [v0.46.0...v0.47.0](https://github.com/sonnnnnnp/dolphin/compare/v0.46.0...v0.47.0) |
| [v0.48.0](https://github.com/sonnnnnnp/dolphin/tree/v0.48.0) | 整数 `int`（`123i`、`to_int`） | [v0.47.0...v0.48.0](https://github.com/sonnnnnnp/dolphin/compare/v0.47.0...v0.48.0) |
| [v0.49.0](https://github.com/sonnnnnnp/dolphin/tree/v0.49.0) | ストリーム `std:stream`（`HTTP` の `"stream": true` を含む） | [v0.48.0...v0.49.0](https://github.com/sonnnnnnp/dolphin/compare/v0.48.0...v0.49.0) |
| [v0.50.0](https://github.com/sonnnnnnp/dolphin/tree/v0.50.0) | プロセスの常駐連携（`os.spawn` `os.wait`） | [v0.49.0...v0.50.0](https://github.com/sonnnnnnp/dolphin/compare/v0.49.0...v0.50.0) |
| [v0.51.0](https://github.com/sonnnnnnp/dolphin/tree/v0.51.0) | ワーカー `std:worker`（別スレッドの別インタプリタ） | [v0.50.0...v0.51.0](https://github.com/sonnnnnnp/dolphin/compare/v0.50.0...v0.51.0) |
| [v0.52.0](https://github.com/sonnnnnnp/dolphin/tree/v0.52.0) | FFI `std:ffi`（C の共有ライブラリを呼ぶ。`--features ffi`） | [v0.51.0...v0.52.0](https://github.com/sonnnnnnp/dolphin/compare/v0.51.0...v0.52.0) |
| [v0.53.0](https://github.com/sonnnnnnp/dolphin/tree/v0.53.0) | バイトコード VM（評価器の置き換え。挙動は同じ） | [v0.52.0...v0.53.0](https://github.com/sonnnnnnp/dolphin/compare/v0.52.0...v0.53.0) |

差分は、まず `docs/design.md`（仕様）から読み、次に `tests/cases/`（使い方の実例）、最後に `src/` を読むと流れがつかめます。手元では `git diff v0.2.0 v0.3.0 --stat` のように見られます。

エラーは該当箇所を指して表示します。

```
runtime_error.dol:5:12: エラー: 添字 5 は範囲外です（要素は 2 個）
  |
5 |     # @arr{5}
  |            ^
  = inner の中（2:7 から呼び出し）
  = outer の中（8:1 から呼び出し）
```

## 実装構成

```
ソースコード → Lexer → Parser → AST → Resolver → Interpreter
```

| 段階 | 役割 |
|---|---|
| Lexer | ソース文字列をトークン列に分割する |
| Parser | トークン列を再帰下降で解析し、AST（構文木）を作る |
| Resolver | 実行前に、未定義の関数・引数の数・宣言されていない変数などを検査する |
| Compiler / VM | AST を命令の列（バイトコード）にコンパイルして、VM で実行する |

全体像は [docs/architecture.md](docs/architecture.md) にまとめています。コア言語は外部クレートに依存しません（HTTP の TLS は、既定で有効な `http` feature の `ureq` を、ハッシュ・HMAC・乱数は `crypto` feature の `ring` と `md-5` を、正規表現は `regex` feature の `regex` を使います）。GUI（`std:ui`）は依存が多いので、`ui` feature（`cargo build --features ui`）を付けたときだけ含まれます。音（`std:audio`）も `audio` feature（`rodio` を使う。Linux では `libasound2-dev` が要ります）です。SQLite（`std:sqlite`）は `sqlite` feature（`rusqlite` で SQLite のソースを同梱する。C コンパイラが要ります）です。FFI（`std:ffi`）は `ffi` feature（`libloading` と `libffi` を使う。C コンパイラが要ります）です。

## ビルド・実行方法

必要なもの: [Rust](https://rustup.rs)（stable）

```sh
# スクリプトを実行
cargo run -- examples/hello.dol

# 引数をつけて実行（スクリプトの中で os.args[] で読める）
cargo run -- examples/hello.dol a b

# 引数なしで対話モード（exit で終了）
cargo run

# 途中経過を見る
cargo run -- --tokens examples/hello.dol   # トークン列
cargo run -- --ast examples/hello.dol      # 構文木（S 式）

# テスト
cargo test
```

`cargo build --release` でビルドすると、実行ファイルは `target/release/dolphin`（Windows は `dolphin.exe`）にできます。

## ブランチ構成

| ブランチ | 内容 |
|---|---|
| `main` | 作り直し版（Rust） |
| [`archive/cpp`](https://github.com/sonnnnnnp/dolphin/tree/archive/cpp) | 初期実装（C++17 + SFML）。サンプルスクリプトは `my_scripts/` |
