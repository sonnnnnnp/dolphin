# 変更履歴

このファイルには、バージョンごとの目立った変更を書きます。
書き方は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います（1.0.0 までは、機能を足すたびに 0.x の x を上げます）。

仕様の詳細は [docs/design.md](docs/design.md)、実装の構成は [docs/architecture.md](docs/architecture.md) にあります。

## [Unreleased]

## [0.19.0] - 2026-10-12

数値リテラル（[design.md 5.19](docs/design.md)）。

### 追加
- 16・2・8 進数 `0xFF` `0b1010` `0o17`、桁区切り `1_000_000`、指数 `1e9` `1.5e-3` `2E+3`。値は今までどおり 64 ビット浮動小数点数で、`0x` などで 2^53 を超える整数、`_` の置き場所の間違い、大きすぎる値は字句エラー
- `dolphin fmt` も新しい数値の書き方をそのまま保つ

## [0.18.0] - 2026-10-12

整形（[design.md 5.18](docs/design.md)）。

### 追加
- `dolphin fmt <file>...`: 空白と字下げを整えて書き戻す。`--check` は書き換えず、整形が必要なら終了コード 1。`--stdout` は結果を標準出力に出す
- 構文木を作り直さず、字句の間の空白と字下げだけを決め直す。コメント・改行の位置・文字列・数値はそのまま。整形の前後で字句が同じことを確かめ、構文エラーのあるファイルは整形しない
- `dolphin::fmt::format_source`
- `tests/fmt_files.rs`: リポジトリの `.dol` をすべて整形して、字句が変わらないことと冪等を確かめる

## [0.17.0] - 2026-10-12

開発の道具（[design.md 5.17](docs/design.md)）。`dolphin fmt` は v0.18.0 に分けた。

### 追加
- `assert[cond]` `assert[cond, message]`、`assert_eq[actual, expected]` `assert_eq[actual, expected, message]`: 失敗は実行時エラー（`?` では拾えない）。`assert_eq` は両方の値を表示する
- `dbg[value]`: 値を標準エラーに `[dbg] 値` で出し、そのまま返す
- `dolphin check <file>...`: 実行せずに構文と Resolver の検査だけをする。問題があれば終了コード 1
- `dolphin test [path...]`: ディレクトリ内の `*_test.dol`（ファイルは直接も可）の、引数 0 個で `test_` で始まる関数を定義の順に実行し、`ok` / `FAIL` と件数を出す。失敗したファイルは `log` の出力も出す。失敗か、テストが 1 つもなければ終了コード 1
- `Session::check`、`Session::user_functions`、`dolphin::testrun`

## [0.16.0] - 2026-10-12

HTTP サーバー（[design.md 5.16](docs/design.md)）。

### 追加
- `std:http` の `serve[port, handler]` / `serve[options, handler]`（`options` は `port` `host` `max_body`）。ハンドラはリクエストのマップ（`method` `path` `query` `headers` `body` `raw` `remote`）を受け取り、`status` `headers` `body` `json` のマップか文字列を返す
- 接続の受け付けと HTTP の解析・応答の書き込みは補助スレッドで行い、解析済みのデータだけをインタプリタのスレッドに送る。ハンドラは 1 つずつ順番に呼ぶ。HTTP/1.1（keep-alive、`Content-Length` と chunked の本文、`Expect: 100-continue`、`HEAD`）の解析は自前で、外部クレートは使わない
- ハンドラのエラーや正しくない戻り値は、`500` を返し、位置と呼び出し履歴を標準エラーに出して、サーバーは続ける。`os.exit` はサーバーごと止める。本文が `max_body` を超えれば `413`、読めないリクエストは `400`
- `tests/http_server.rs`: `dolphin` を子プロセスで起動して HTTP で叩くテスト
- `examples/todo_api.dol`: JSON でメモを出し入れする小さな API サーバー

## [0.15.0] - 2026-10-12

メモリ回収（[design.md 5.15](docs/design.md)）。

### 追加
- 循環回収器（`src/runtime/gc.rs`）。配列・マップ・スコープ・関数値を弱参照で登録し、参照カウントだけを使って、外から届かない循環を見つけて中身を空にする（Bacon–Rajan 型の試行削除）。それまでは、関数値が自分のスコープを持つような循環は解放されず、長く動かすと漏れていた
- 作った対象が 10000（生き残りが多ければその数）に達すると、関数呼び出しの入口とループの 1 周の先頭で自動で回収する
- `gc_collect[]`: すぐに回収し、回収した対象の数を返す

## [0.14.0] - 2026-10-12

ここまでの機能を使って、大きめのプログラムを書いてみた。

### 追加
- `examples/dolvc.dol`: Dolphin で書いた小さなバージョン管理ツール（`init` `commit` `status` `diff` `log` `checkout`）。ファイルの中身を sha256 で名前づけして保存し、コミットは JSON。行ごとの差分は最長共通部分列で計算する。ファイル・ハッシュ・JSON・時刻・パス・バイト列・エラー値と `?`・クロージャを使っている
- `examples/dolvc_demo.dol`: `dolvc` を一時ディレクトリで通して動かすデモ（自分自身を `os.run` で子プロセスにして動かす）
- `examples/life.dol`: ライフゲーム（グライダー銃）のアニメーション
- `examples/mandelbrot.dol`: マンデルブロ集合を文字で描く
- `os.exe[]`: 実行中の `dolphin` の実行ファイルのパス
- `tests/examples.rs`: すべての例が構文として正しいことと、`dolvc_demo` `life` `mandelbrot` の動きを確かめる

## [0.13.0] - 2026-10-12

バイト列と暗号（[design.md 5.14](docs/design.md)）。

### 追加
- バイト列の型 `bytes`（変更できない。`type_of` は `"bytes"`、表示は `<bytes 68690a>`）
  - 作る: `str_to_bytes` `bytes_from_arr` `bytes_new` `bytes_concat`。読む: `bytes_len` `@b{i}` `bytes_slice` `bytes_to_arr` `bytes_find` `bytes_to_str`（UTF-8 でなければ `parse` のエラー値）
  - `==` で中身を比べられる。書き換え（`@b{0} = 1`）は実行時エラー。JSON にはできない
- バイナリのファイル入出力 `file_read_bytes` `file_write_bytes` `file_append_bytes`
- `std:http` の `body` にバイト列を渡せる。`"as": "bytes"` でレスポンスの本文をバイト列で受け取れる（UTF-8 でない本文もエラーにならない）
- `std:os` の `run` の `stdin` にバイト列を渡せる。`"as": "bytes"` で `stdout` と `stderr` をバイト列で受け取れる
- `std:encoding`（自前の実装）: `base64_encode` `base64_decode` `base64url_encode` `base64url_decode` `hex_encode` `hex_decode` `url_encode` `url_decode`。引数はバイト列か文字列。正しくない入力は `parse` のエラー値
- `std:crypto`（`crypto` feature、既定で有効。暗号は自前で書かず `ring` と `md-5` を使う）: `hash`（`sha1` `sha256` `sha384` `sha512` `md5`）`hmac` `random_bytes` `uuid`（v4）`equals`（時間が中身によらない比較）

## [0.12.0] - 2026-10-12

システムまわり（[design.md 5.13](docs/design.md)）。

### 追加
- ファイルの関数 `file_list` `file_info` `file_mkdir` `file_remove` `file_remove_all` `file_rename` `file_copy`。失敗はエラー値（種類に `exists` を追加）
- `std:path`: `join` `dirname` `basename` `stem` `ext` `normalize` `is_absolute` `absolute`、変数 `sep`。出力は常に `/` 区切りで、システムによらず同じ結果になる
- `std:os`: `args` `env` `env_all` `cwd` `chdir` `exit` `platform` `temp_dir` `run`
  - `run[command, args, options]` は外部コマンドを実行して待つ（シェルは通さない）。`cwd` `env` `stdin` `timeout` を指定でき、`{"status", "ok", "stdout", "stderr"}` を返す。起動の失敗・時間切れ・UTF-8 でない出力はエラー値
  - `exit[code]` はその場でスクリプトを終了し、出力を書き出してから終了コードを返す（関数の中・コールバックの中・import したファイルの中からも終われる）
- `std:time`（外部クレートなしの自前の暦の計算）: `now` `monotonic` `sleep` `date` `timestamp` `format` `parse` `iso` `parse_iso`
  - 時刻は Unix 時刻（秒）、時差は UTC からの分。年は 1〜9999、グレゴリオ暦。システムの時差は扱わない
  - `format` / `parse` の書式は `%Y %m %d %H %M %S %f %y %e %j %a %A %b %B %p %I %z %%`
- コマンドライン引数。`dolphin script.dol a b` と書くと、`os.args[]` が `{"a", "b"}` になる
- 組み込みモジュールが文字列の定数を持てるようになった（`std:path` の `sep`）

## [0.11.0] - 2026-10-12

基本型の関数の穴をふさいだ（[design.md 5.12](docs/design.md)）。配列・マップをその場で書き換えるのは `push` `pop` `insert` `remove` `clear` だけで、ほかは新しい値を返す。

### 追加
- `type_of[v]`（`"nil"` `"bool"` `"number"` `"string"` `"array"` `"map"` `"function"` `"module"` `"error"`）
- 配列: `arr_slice` `arr_concat` `arr_reverse` `arr_sort`（数値だけか文字列だけ。安定）`arr_sort_by`（比較関数つき。安定。矛盾した比較関数でもパニックしない自前のマージソート）`arr_index_of` `arr_contains` `arr_find` `arr_find_index` `arr_any` `arr_all` `arr_reduce` `arr_insert` `arr_remove` `arr_clear` `arr_copy`
- マップ: `map_values` `map_items` `map_get`（既定値つき）`map_merge` `map_clear` `map_copy`
- 文字列: `str_starts_with` `str_ends_with` `str_repeat` `str_pad_left` `str_pad_right` `str_trim_start` `str_trim_end` `str_lines` `str_code_at` `str_from_code` `str_rfind`
- 数値: `num_fixed`（小数点以下の桁数を指定した文字列。ちょうど半分は 0 から遠いほうへ丸める）`trunc` `sign` `clamp`
- `std:math`: 定数 `pi` `e` `inf` `nan`（組み込みモジュールが定数の変数を持てるようになった）、三角関数、`exp` `ln` `log10` `log2` `hypot`、`is_nan` `is_finite`。IEEE 754 に従い、定義できない入力には `nan` や `inf` を返す
- 文字列のエスケープ `\t` と `\r`
`

## [0.10.0] - 2026-10-12

### 追加
- HTTP クライアント `std:http`（[design.md 5.11](docs/design.md)）。`ureq`（TLS は rustls）を使う、ブロッキングの実装
  - `request[options]`、`get[url]`、`post[url, options]`、`put`、`delete`
  - `options` は `headers` `query` `body` `json` `timeout` `redirects`。`json` は JSON にして送り、`Content-Type` も付ける
  - 戻り値は `{"status", "ok", "headers", "body"}` のマップ。`404` や `500` もエラーにせず、普通のレスポンスとして返す
  - 通信の失敗はエラー値（種類は `timeout` `url` `request` `too_large` `parse` `network`）。`?` で伝えられる
  - オプションの間違い（知らないキー、型の違い）は、プログラムの間違いとして実行時エラー
- `http` feature（既定で有効）。`--no-default-features` ならビルドに入らず、`import["std:http"]` は「含まれていません」と案内する。CI で両方のビルドを検査する

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
