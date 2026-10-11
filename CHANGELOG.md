# 変更履歴

このファイルには、バージョンごとの目立った変更を書きます。
書き方は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います（1.0.0 までは、機能を足すたびに 0.x の x を上げます）。

仕様の詳細は [docs/design.md](docs/design.md)、実装の構成は [docs/architecture.md](docs/architecture.md) にあります。

## [Unreleased]

## [0.9.0] - 2026-10-11

### 追加
- 組み込みモジュール。`import["std:名前"]` で、ファイルではなく組み込みのモジュールを読み込む。使い方は通常のモジュールと同じ（`@j.parse[...]`）。存在しない名前は、あるものを案内するエラーになる（[design.md 5.9](docs/design.md)）
- `std:json`（外部クレートなしの自前の実装。[design.md 5.10](docs/design.md)）
  - `parse[s]`: JSON の文字列を値にする。`null` は `nil`、オブジェクトは追加順を保つマップ。正しくなければ、何行何列目かを含む `parse` のエラー値
  - `stringify[v]`: 1 行の JSON にする。`pretty[v]`: インデント 2 つで整形する
  - 関数・エラー値・NaN・循環した配列やマップは、JSON にできないものとして実行時エラー

## [0.8.0] - 2026-10-11

エラー処理（[design.md 8.6](docs/design.md)）。外の世界の失敗は**エラー値**で返し、プログラムの間違いは今までどおり実行時エラーで止める。

### 追加
- エラー値（新しい型）。種類（`"not_found"` など）とメッセージを持つ。`<エラー not_found: メッセージ>` と表示される
- 組み込み関数 `is_error` `err_kind` `err_message` `err_new`
- 後置の `?`。値がエラーなら、いまの関数（無名関数を含む）からそのエラーを返し、そうでなければ値になる。関数の外で使ってエラーになったら、そのメッセージで止まる

### 変更（互換性なし）
- `file_read` `file_write` `file_append` が、失敗したときに実行時エラーではなくエラー値を返す（種類は `not_found` `permission` `io` `parse`）
- `to_num` が、数値に読めない文字列に対して `parse` のエラー値を返す（型の間違いは今までどおり実行時エラー）
- エラー値を式文として捨てると、実行時エラーになる（`file_write[...]` の失敗に気づかないまま進まないため）。ファイルの最後の式文も同じ
- エラー値を `if` の条件や演算に使うと、型の間違いとして止まる

## [0.7.0] - 2026-10-11

### 追加
- `nil` リテラル。「値なし」を書けるようになった。JSON の `null` に当たる（[design.md 5.3](docs/design.md)）
- `for @x in 配列 ( … )`。要素を順に回す。本体は繰り返しごとに新しいスコープで、`break` と `continue` も使える（[design.md 5.6](docs/design.md)）
  - マップは `for @k in map_keys[@m]`、回数指定は `for @i in range[0, 10]`
- 組み込み関数 `range[a, b]`（`a` 以上 `b` 未満の整数の配列）

### 変更
- `break` / `continue` のエラーメッセージを「while か for の中でのみ使えます」にした

## [0.6.0] - 2026-10-11

### 追加
- 関数値とクロージャ（[design.md 5.8](docs/design.md)）
  - 無名関数 `[@n] ( # @n + 1 )`。作られた場所のスコープを参照で捕まえる（外側の変数を書き換えると関数からも見える）
  - 関数名だけを書くと関数値（`@f := add`、組み込み関数も同じ）。`@u.add`（`[` なし）でモジュールの関数も取り出せる
  - 変数の関数値は `@f[1, 2]` で呼ぶ。関数を返す関数（`adder[1][2]`）も書ける
  - 関数や無名関数を呼ぶとき、引数の数を実行時に検査する
  - 配列に `[ ]` を使ったときは「添字は `{ }` で読みます」と案内する
- 組み込み関数 `arr_map` `arr_filter` `arr_each`。渡した関数の中で起きたエラーは、呼び出し履歴つきで報告する

## [0.5.0] - 2026-10-11

デスクトップアプリや実用的な言語に向けて、土台になる文法を見直した（[design.md 8 章](docs/design.md)）。**今までのスクリプトは書き換えが必要**。

### 変更（互換性なし）
- 変数の印を `@` に統一した。`$` は廃止（使うとエラー）。グローバルかローカルかは、宣言された場所で決まる
- 宣言と代入を分けた。宣言は `@x := 1`、代入は `@x = 2`。宣言していない変数への代入は、実行前エラーになる（タイプミスを検出できる）
- 添字を `@a[0]` から `@a{0}` に変えた。`[ ]` は呼び出し専用になった。`@m{"a"}{"b"} = 1` のように続けて書ける
- スコープは `( )` のブロックと関数ごとに 1 つになった。内側で同じ名前を宣言すると外側を隠す。同じスコープでの二重宣言はエラー
- 関数の中からは、そのファイルのトップレベルの変数を読み書きできる（宣言が関数より後ろにあってもよい）
- 文字列の中の `$` は、ただの文字になった（`\$` のエスケープは不要）

## [0.4.0] - 2026-10-11

### 追加
- ファイル入出力の組み込み関数 `file_read` `file_write` `file_append` `file_exists`（[design.md 5.5](docs/design.md)）
  - パスは実行したときのカレントディレクトリからの相対パス。文字コードは UTF-8
  - 失敗したときは、行・列つきのエラーになる

## [0.3.0] - 2026-10-11

### 追加
- `import`（名前空間つき）。`@u = import["util.dol"]` で読み込み、`@u.add[1, 2]` で関数を呼び、`@u.version` で `@` 変数を読める（[design.md 5.7](docs/design.md)）
  - `@` 変数と関数はファイルごとに別で、ほかのファイルとぶつからない
  - 同じファイルを何度 import しても、実行は 1 回だけ
  - 外から `@u.name = v` と書き換えることはできない（構文エラー）
  - 循環 import はエラーにする
  - import したファイルで起きたエラーは、そのファイルの行を引用し、import した位置を呼び出し履歴に載せる
- 値の型にモジュールを追加
- 記号 `.`

## [0.2.0] - 2026-10-11

### 追加
- マップ（連想配列）。`{"name": "Dolphin"}`、`@m["key"]` での読み書き、`map_len` `map_keys` `map_has` `map_remove`
- 配列とマップを参照渡しにした（[design.md 7.1](docs/design.md)）
- 文字列の組み込み関数 `str_upper` `str_lower` `str_trim` `str_contains` `str_find` `str_replace` `str_split` `str_join` `str_sub`
- 配列の `arr_pop`
- 変換 `to_str` `to_num`
- 数学の関数 `abs` `floor` `ceil` `round` `sqrt` `pow` `min` `max` `random` `random_int`
- 対話モード（REPL）。引数なしで `dolphin` を起動すると使える。複数行の入力、式の値の表示、履歴に対応
- `while` の中で使える `break` と `continue`

### 変更
- 自分自身を含む配列・マップを、無限に表示し続けず `{...}` と表示する
- README を Rust 版の内容に書き直した

## [0.1.0] - 2026-10-06

C++ 版から Rust での作り直しを始めた最初のバージョン。

### 追加
- 処理系の土台: Lexer → Parser → AST → Resolver → tree-walking 評価器
- 変数（`@` グローバル、`$` ローカル）、四則演算・剰余・比較・論理演算（短絡評価）、単項 `-` と `!`、`( )` によるグループ化
- `if` / `else` / `else if`、`while`
- 関数（定義より前の行から呼べる、再帰、`#` で戻り値）
- 数値、文字列、真偽値、配列。文字列の中の `@x` `$x` の展開
- 組み込み関数 `log` `input` `str_len` `str_concat` `arr_len` `arr_push`
- 行・列と呼び出し履歴つきのエラー表示。実行前の検査（未定義の関数、引数の数、関数の外での `$x` など）
- ゴールデンテスト（`tests/cases/*.dol` と期待出力の比較）

[Unreleased]: https://github.com/sonnnnnnp/dolphin/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/sonnnnnnp/dolphin/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/sonnnnnnp/dolphin/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/sonnnnnnp/dolphin/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sonnnnnnp/dolphin/releases/tag/v0.1.0
