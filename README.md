# 🐬 Dolphin

[English](README.en.md)

`@` で変数を宣言する、小さなインタプリタ言語。現在 Rust で作り直し中（旧実装は C++）。拡張子は `.dol`。

## このプロジェクトについて

インタプリタ言語がどう動いているのかを理解するために、インタプリタ言語を自作しています。せっかく自作するので、シンタックスはユニークにしました。

作る理由は、学習とロマンと自己満足です。実用を目的にはしていませんが、ロマンとして実際に使っても困らない水準を目標にしています。バックエンドの API やデスクトップアプリも作れる言語を目指しています。

```
@name = Dolphin
log[Hello, @name!]
add[$a, $b] (
    # $a + $b
)
@i = 1
while @i <= 3 (
    log[count: @i]
    @i = add[@i, 1]
)
```

実行結果:

```
Hello, Dolphin!
count: 1
count: 2
count: 3
```

## なぜ `@` なのか

<!-- ここに書く -->

## できること

- [x] `@` による変数の宣言・代入・参照（数値・文字列）
- [x] 四則演算と剰余（`+` `-` `*` `/` `%`）
- [x] 比較演算（`==` `!=` `>` `<` `>=` `<=`）、論理演算（`&&` `||`）
- [x] `if` / `else`
- [x] `while`
- [x] 配列（`@nums = {10, 20, 30}`、`@nums[0]` で読み取り・代入）
- [x] `log[...]` による出力（文字列中の `@変数` を展開）
- [x] `input[@var]` による標準入力
- [x] ユーザー定義関数（`$` で引数・ローカル変数、`#` で戻り値）
- [x] 配列・文字列の組み込み関数（`arr_len` `arr_set` `arr_push` `str_concat` `str_len`）
- [x] SFML を使ったウィンドウ描画（矩形・円・画像・テキスト・キー入力・マウス入力・サウンド）
- [x] 行頭の `//` コメント
- [x] 引数なしで起動したときの REPL モード
- [ ] 式の中の括弧によるグループ化（`(@a + @b) * 2` など）
- [ ] 行末コメント
- [ ] エラー発生時の行番号表示
- [ ] バックエンド API を作るための HTTP サーバー機能

組み込み関数の一覧と詳しい構文は [`archive/cpp` の docs/README.md](https://github.com/sonnnnnnp/dolphin/blob/archive/cpp/docs/README.md) にあります。

## 実装構成

```
ソースコード → Lexer → Parser → AST → Evaluator
```

| 段階 | 役割 |
|---|---|
| Lexer | ソース文字列をトークン列に分割する |
| Parser | トークン列から構文を解析する |
| AST | 文と式を木構造で表す |
| Evaluator | AST を木たどりで評価する |

初期実装（`archive/cpp`）は AST を作らず、ソースを1行ずつ読み、文字列のまま式を分割して評価する方式です。主なファイルは次のとおりです。

| ファイル | 内容 |
|---|---|
| `src/main.cpp` | ファイル実行と REPL の入口 |
| `src/interpreter/DolphinInterpreter.cpp` | 文の実行、式の評価、ユーザー定義関数 |
| `src/interpreter/Builtins.cpp` | 入出力・配列・文字列の組み込み関数 |
| `src/interpreter/GraphicsBuiltins.cpp` | SFML を使った描画・入力・サウンドの組み込み関数 |

## ビルド・実行方法

必要なもの: CMake 3.16 以上、C++17 対応コンパイラ。SFML 2.6.2 はビルド時に自動で取得されます。

```sh
git switch archive/cpp
cmake -S . -B build
cmake --build build
```

```sh
# スクリプトを実行
./build/dolphin my_scripts/test.dol

# 引数なしで REPL を起動（exit で終了）
./build/dolphin
```

Visual Studio のジェネレータでは、実行ファイルは `build/Debug/dolphin.exe` に出力されます。

## ブランチ構成

| ブランチ | 内容 |
|---|---|
| `main` | 作り直し中の実装（Rust） |
| [`archive/cpp`](https://github.com/sonnnnnnp/dolphin/tree/archive/cpp) | 初期実装（C++17 + SFML）。サンプルスクリプトは `my_scripts/` |
| [`archive/rust`](https://github.com/sonnnnnnp/dolphin/tree/archive/rust) | 初期実装を Rust + macroquad に移植したもの |
