# 🐬 Dolphin

[English](README.en.md)

`@` で変数を宣言する、Rust 製の小さなインタプリタ言語。拡張子は `.dol`。

## このプロジェクトについて

インタプリタ言語がどう動いているのかを理解するために、インタプリタ言語を自作しています。せっかく自作するので、シンタックスはユニークにしました。

作る理由は、学習とロマンと自己満足です。実用を目的にはしていませんが、ロマンとして実際に使っても困らない水準を目標にしています。バックエンドの API やデスクトップアプリも作れる言語を目指しています。

```
@name = "Dolphin"
log["Hello, @name!"]

add[$a, $b] (
    # $a + $b
)

@i = 1
while @i <= 3 (
    log["count: @i"]
    @i = add[@i, 1]
)

@user = {"name": @name, "langs": {"Rust", "C++"}}
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
| `@x = 1` | グローバル変数 |
| `$x = 1` | ローカル変数（関数の中だけ） |
| `name[a, b]` | 関数呼び出し |
| `name[$a, $b] ( … )` | 関数定義 |
| `# value` | 関数から値を返す |
| `if 条件 ( … )` / `else ( … )` / `else if` | 条件分岐 |
| `while 条件 ( … )` / `break` / `continue` | 繰り返し |
| `{1, 2, 3}` / `@a[0]` | 配列 |
| `{"key": value}` / `@m["key"]` / `{:}` | マップ（`{:}` は空のマップ） |
| `"Hi, @name"` / `"$x"` | 文字列。中の変数は展開される |
| `// コメント` | 行末までコメント |

詳しい仕様は [docs/design.md](docs/design.md)、組み込み関数の一覧は [design.md 5.5](docs/design.md#55-組み込み関数) にあります。

## できること

段階は [docs/design.md](docs/design.md) の 2.1 に対応しています。

- [x] **段階 1: コア言語**
  - 変数（`@` グローバル / `$` ローカル）、四則演算・剰余・比較・論理演算（短絡評価）、単項 `-` `!`、`( )` によるグループ化
  - `if` / `else` / `else if`、`while`、`break` / `continue`
  - 関数（定義より前から呼べる、再帰、`#` で戻り値）
  - 配列、文字列の中の `@x` `$x` の展開、行末コメント
- [x] **段階 2: 実用化**
  - 行・列と呼び出し履歴つきのエラー表示
  - マップ（連想配列）
  - 文字列・数値変換・数学の組み込み関数
  - 対話モード（REPL）
- [ ] **段階 3**: 複数ファイル、ファイル入出力
  - [x] `import`（名前空間つき。`@u = import["util.dol"]`、`@u.add[1, 2]`）
  - [ ] ファイル入出力
- [ ] **段階 4**: HTTP サーバー、JSON → バックエンド API
- [ ] **段階 5**: ウィンドウ・描画・入力 → デスクトップアプリ（旧 C++ 実装では SFML で対応済み）

エラーは該当箇所を指して表示します。

```
runtime_error.dol:5:12: エラー: 添字 5 は範囲外です（要素は 2 個）
  |
5 |     # $arr[5]
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
| Resolver | 実行前に、未定義の関数・引数の数・変数を使える場所などを検査する |
| Interpreter | AST を木たどりで評価する |

全体像は [docs/architecture.md](docs/architecture.md) にまとめています。コア言語は外部クレートに依存しません。

## ビルド・実行方法

必要なもの: [Rust](https://rustup.rs)（stable）

```sh
# スクリプトを実行
cargo run -- examples/hello.dol

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
| [`archive/rust`](https://github.com/sonnnnnnp/dolphin/tree/archive/rust) | 初期実装を Rust + macroquad に移植したもの |
