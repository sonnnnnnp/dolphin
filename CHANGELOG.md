# 変更履歴

このファイルには、バージョンごとの目立った変更を書きます。
書き方は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います（1.0.0 までは、機能を足すたびに 0.x の x を上げます）。

仕様の詳細は [docs/design.md](docs/design.md)、実装の構成は [docs/architecture.md](docs/architecture.md) にあります。

## [Unreleased]

## [0.48.0] - 2026-10-20

整数 `int`（[design.md 5.40](docs/design.md)）。数値は 64 ビット浮動小数点数で、2^53 を超える整数（64 ビットの ID やハッシュ、ナノ秒の時刻）が正確に扱えなかったため。今までの数値のコードは変わらない。

### 追加
- 整数型 `int`（64 ビット符号付き）。リテラルは `123i` `0xFFi` `0b101i` `0o17i` `1_000i`。`type_of` は `"int"`
- 整数どうしの `+ - *` と単項 `-` は整数、範囲を出たら実行時エラー（折り返さない）。`/` は 0 の方向への切り捨て、`%` は被除数と同じ符号の余り。比較も整数どうし
- 整数と数値の演算・大小比較は実行時エラー（`1i == 1` は false）。変換は `to_int[v]`（数値・文字列から。できなければ `parse` のエラー値）と、整数も受ける `to_num`
- 整数を配列・バイト列の添字に使える。`abs` `sign` `min` `max`、`arr_sort`、`arr_sort_by` の比較関数の戻り値が整数に対応。`std:bits` は引数が整数なら 64 ビットのまま計算して整数を返す
- `json.stringify` は整数を整数のまま書く。`json.parse[s, {"int": true}]` で小数点も指数もない整数を整数で読む。`sqlite` のパラメータに整数を渡せ、`sqlite.open[path, {"int": true}]` で INTEGER が整数で返る
- 整数を、イベントループなどスレッドをまたぐ値に載せられる

### 変更
- 2^53 を超える数値リテラルのエラーに、整数にするなら `i` を付ける案内を足した
- `arr_sort` `arr_sort_by` と `sqlite` のパラメータの型の間違いのメッセージに、整数が加わった

## [0.47.0] - 2026-10-18

エラー値の詳細データ（[design.md 5.39](docs/design.md)）。ライブラリが、HTTP のステータスやエラーコードをエラー値に載せられるように。

### 追加
- `err_new[kind, message, data]`（第 3 引数）と `err_data[e]`: エラー値に任意の詳細データ（持ち出せる値）を持たせる・取り出す。なければ `nil`。標準ライブラリのエラーは今までどおり詳細なし。`==` は詳細も含めて比べる。表示は変わらない

## [0.46.0] - 2026-10-18

パッケージの `import`（[design.md 5.38](docs/design.md)）。別のリポジトリで配るライブラリ（yaylib の Dolphin 版など）を使うため。

### 追加
- `import["名前"]` `import["名前/モジュール"]`: `.` や `/` で始まらない名前は、`import` したファイルのあるディレクトリから上の階層の `dolphin_modules/` と、環境変数 `DOLPHIN_PATH` の各ディレクトリから探す。`dolphin_modules/名前/mod.dol`（または `dolphin_modules/名前.dol`）、`dolphin_modules/名前/モジュール.dol`。配るのは `git clone` や git submodule に任せ、バージョン管理やダウンロードは作らない
- 言語サーバーの定義へ移動・ホバー・補完も、パッケージの中を探す

## [0.45.0] - 2026-10-18

WebSocket のコールバック版クライアント（[design.md 5.37](docs/design.md)）。ブロッキングの `recv` だと、待っている間タイマーもほかの通信も止まるため。

### 追加
- `ws.connect_async[url, handlers]` `ws.connect_async[url, options, handlers]`: 接続の読み書きは補助スレッドが行い、`on_open[conn]` `on_message[conn, message]` `on_close[conn, code, reason]` `on_error[error]` をイベントループ（`std:async`）の上で 1 つずつ呼ぶ。戻り値の番号を `async.cancel` に渡すと、静かに閉じる。`wss://` も使える。`options` は `ws.connect` と同じ（`headers` `timeout`）に、`ping_interval`（秒）が加わる
- 開いている間は、イベントループの「待っているもの」に数える。再接続は自動ではしない（`on_close` の中で `async.after` を使って呼び直す）

### 変更
- `ws.recv` と `ws.set_timeout` をコールバック式の接続に使ったときのメッセージを、`ws.connect_async` も含む言い方にした

## [0.44.0] - 2026-10-17

画像の加工（[design.md 5.36](docs/design.md)）。画像を投稿する前に、縦横を調べてサムネイルを作るため。

### 追加
- `import["std:image"]`（`image` feature、既定で有効。`ui` feature にも含まれる）: `info`（形式は先頭のバイトから決め、ヘッダーだけ読んで縦横を返す）、`resize`（`fit` は `exact` `inside`〔縦横比を保ち、小さい画像は拡大しない〕`cover`、`filter` は `bilinear` `nearest` `lanczos`）、`crop`、`convert`。形式は `png` `jpeg`〔`quality` 1〜100、既定 85〕`gif`。入力は `bmp` `webp` も読める
- 動く GIF は、GIF で出すとき全フレームを加工して、表示時間を保つ。透明は JPEG にするとき白の上に重ねる
- 縦横 16384 画素・1 億画素を超える画像は `too_large` のエラー値

## [0.43.0] - 2026-10-17

`wss://`（[design.md 5.35](docs/design.md)）。yaylib のように、TLS でしか話せない WebSocket のサーバーにつなぐため。

### 追加
- `ws.connect` が `wss://host[:port]/path` を受ける（ポートの既定は 443）。TLS は rustls、ルート証明書は `std:http` と同じ webpki-roots。証明書とホスト名の検証は常に行う（期限切れなどは `network` のエラー値）。`send` `recv` `close` `set_timeout` は `ws://` と同じ
- `tls` feature（既定で有効）。外した版では `wss://` が「この版には含まれていません」のエラー値になる

### 変更
- WebSocket の `Host` ヘッダーは、ポートが既定（80・443）なら付けない。IPv6 は `[::1]` の形で送る

## [0.42.0] - 2026-10-17

言語サーバーの続き（[design.md 5.34](docs/design.md)）。

### 追加
- `dolphin lsp` に**補完**（`import["std:` のモジュール名、`@m.` のメンバー、`@` の変数、関数・組み込み関数・キーワード。構文エラーの間は直前に解析できた結果を使う）、**参照の検索**、**リネーム**（`prepareRename` つき。開いているファイルの中だけ。名前がぶつかるとき、組み込み・ほかのファイルの宣言のときは断る）を追加
- 文字列の中の `@{ 式 }` の名前も、定義へ移動・ホバー・リネームの対象にした
- `Session::catalog()`（組み込み関数と組み込みモジュールのメンバーの名前の一覧）

## [0.41.0] - 2026-10-17

言語サーバー（[design.md 5.33](docs/design.md)）。

### 追加
- `dolphin lsp`: 標準入出力で LSP を話す言語サーバー。**定義へ移動**（変数・引数・関数、`@m := import["相対パス"]` した先の `@m.f`）、**エラー表示**（`dolphin check` と同じ検査）、**ホバー**（関数の形と直前の `//` コメント、変数の宣言の行）、**アウトライン**、**整形**（`dolphin fmt` と同じ）。位置は UTF-16 に変換するので、日本語・絵文字のあとでもずれない。外部クレートなし（JSON-RPC 用に小さな JSON を自前で持つ）
- VS Code 拡張（`editors/vscode`、0.2.0）が `dolphin lsp` を起動する。設定 `dolphin.path` で実行ファイルの場所を変えられる。`npm run package` で `.vsix` を作る

## [0.40.0] - 2026-10-16

コレクションの道具（[design.md 5.32](docs/design.md)）。「Python に比べて足りないもの」の 5 つめ（これで 1 番の 5 つが揃った）。

### 追加
- `import["std:collections"]`（標準ライブラリだけ。feature なし）: ヒープ `heap_push` `heap_pop` `heap_peek` `heapify`（普通の配列をその場でヒープとして使う。順序は既定で数値同士・文字列同士、または `arr_sort_by` と同じ比較の関数）、`counts`（個数）`unique`（重複除去）`group_by`、`chunk` `zip` `flatten`
- 比較の関数の中でヒープを書き換えても、落ちずにエラーになる（比べるたびに短く借り、借りたまま関数を呼ばない）
- 集合型・`deque`・タプルは作らない（設計書 5.32 に理由）

## [0.39.0] - 2026-10-16

乱数（[design.md 5.31](docs/design.md)）。「Python に比べて足りないもの」の 4 つめ。

### 追加
- `import["std:random"]`（標準ライブラリだけ。feature なし）: `seed[n]`（`random[]` `random_int[…]` と共通の乱数列の種を決めて、結果を再現できる。`seed[]` は時刻から決め直す）`choice[items]` `shuffle[items]`（新しい配列）`sample[items, k]`（重ならない `k` 個）`uniform[a, b]` `normal[mean, stddev]`（Box–Muller）。暗号用の乱数は引き続き `std:crypto` の `random_bytes`

### 変更
- `random_int[min, max]` を、剰余の偏りが出ない方式（棄却法）にした。`max - min + 1` が 64 ビットに収まらない範囲でも動く

## [0.38.0] - 2026-10-16

日付の計算（[design.md 5.30](docs/design.md)）。「Python に比べて足りないもの」の 3 つめ。日付の分解・書式・読み取りは v0.12.0 の `std:time` にあったので、足りなかった暦の計算だけを足した。

### 追加
- `std:time` に `is_leap[year]` `days_in_month[year, month]` `add_months[ts, n, offset]`（日が月末を超えるなら月末にする。時刻は変えない。範囲外はエラー値 `parse`）`start_of[ts, unit, offset]`（`unit` は `"year"` `"month"` `"day"` `"hour"` `"minute"` `"second"`）
- システムの時差は、OS の API が要るので、今回も扱わない（`offset` を自分で渡す）

## [0.37.0] - 2026-10-16

CSV（[design.md 5.29](docs/design.md)）。「Python に比べて足りないもの」の 2 つめ。

### 追加
- `import["std:csv"]`（標準ライブラリだけ。feature なし）: `parse[text, options]`（文字列の配列の配列。`header: true` で 1 行目を見出しにしたマップの配列）`stringify[rows, options]`（配列かマップの行の配列を文字列に。マップは `columns` で列を選べる）。オプションは `delimiter`（TSV など）`header` `columns` `newline`
- RFC 4180 に沿う: `"` で囲んだフィールド（中の `""`、区切り・改行）、`
` と `
`、先頭の BOM、空の行の読み飛ばし。失敗はエラー値（`parse`。行番号つき）

## [0.36.0] - 2026-10-16

SQLite（[design.md 5.28](docs/design.md)）。「Python に比べて足りないもの」を埋める 1 つめ。

### 追加
- `import["std:sqlite"]`（`sqlite` feature。**既定では無効**で、`cargo build --features sqlite`。`rusqlite`（`bundled`）で SQLite のソースを同梱するので、利用者が SQLite を別に入れる必要はない。C コンパイラが要る）: `open[path, options]`（`read_only` `timeout`。`":memory:"` でメモリ上の DB）`close` `exec[c, sql, params]`（`{"changes", "last_id"}` を返す）`query[c, sql, params]`（行を、列名をキーにしたマップの配列で返す）`run_script[c, sql]`
- `params` は配列（`?` に順番に）かマップ（`:name` `@name` `$name` に名前で）。`nil`・真偽値・数値・文字列・バイト列を渡せる。整数の数値は INTEGER、そうでなければ REAL
- 失敗はエラー値（種類 `sql` `busy` `closed` `io`）。接続は `handle` 型で、参照されなくなれば自動で閉じる。トランザクションは `exec[c, "BEGIN"]` などの SQL で書く
- CI に `--features sqlite` のビルドとテストを足した

## [0.35.0] - 2026-10-12

音（[design.md 5.27](docs/design.md)）。G の最後。

### 追加
- `import["std:audio"]`（`audio` feature。**既定では無効**で、`cargo build --features audio`。`rodio`（cpal + symphonia）を使う。Linux では `libasound2-dev` が要る）: `play[path, options]`（`volume` `speed` `loop`。音の番号を返す）`stop` `stop_all` `pause` `resume` `set_volume` `set_master_volume` `is_playing`。WAV・OGG・MP3・FLAC
- 再生は補助スレッドが受け持ち、インタプリタは待たない。ファイルを開いて形式を確かめるところまでは `play` の中で行い、失敗（`not_found` `io` `parse`、出力機器がなければ `audio`）はエラー値で返す
- 出力機器がなくても通る単体テスト（引数・ファイルの検査、音の番号の管理。機器があれば再生と停止も）

## [0.34.0] - 2026-10-12

画像（[design.md 5.26](docs/design.md)）。`canvas` に画像を足す。

### 追加
- `canvas.image[path, x, y]` `canvas.image[path, x, y, w, h]` `canvas.image[path, x, y, w, h, options]`（`options` は `src`（画像の一部。スプライトシート用）`tint` `flip_x` `flip_y` `smooth`（`false` で最近傍補間））と、`canvas.image_size[path]`（ヘッダーだけ読んで大きさを返す。失敗はエラー値）
- 画像のデコードと GPU への転送は GUI 側で行い、インタプリタは、ファイルのパスで画像を指す。同じ画像は使い回し、読めない画像は、最初の 1 回だけ標準エラーに出して描かない。PNG・JPEG・GIF（最初のコマ）・BMP・WebP（`image` クレート。`ui` feature に含む）

## [0.33.0] - 2026-10-12

`canvas`（[design.md 5.25](docs/design.md)）。G の続き。`ui` feature に含む。

### 追加
- `import["std:canvas"]`: `canvas.run[options]` でウィンドウを開き、フレームごとに `update[dt]` と `draw[]` を呼ぶ。`options` は `draw`（必須）`update` `title` `width` `height`（論理的な画面の大きさ。ウィンドウに合わせて縦横比を保って拡大・縮小）`background` `fps`（上限。既定 60）`font`
- 描画（`draw` の中）: `clear` `rect` `circle` `line` `polygon` `text`。色は `"#rrggbb"` か `"#rrggbbaa"`、`{"stroke": 太さ}` で線だけ描く
- 入力・画面の情報: `key_down` `key_pressed` `key_released`（小文字のキー名。知らない名前は実行時エラー）`mouse[]`（座標・ボタン・ホイール）`size[]` `quit[]`
- インタプリタが追いつかなければ前のフレームを描いたまま待つ（フレームを溜めない）。`update` / `draw` のエラーは標準エラーに出して続け、同じメッセージは 1 回だけ出す。`async` のタイマーなどと一緒に使える
- `examples/breakout.dol`（ブロック崩し）
- 命令の組み立て・入力の関数・フレームの往復・オプションの検査を、GUI なしで確かめる単体テスト

### 変更
- ウィンドウを開けるのは、`ui.run` と `canvas.run` を合わせて 1 回だけ（ホストの `claim`）

## [0.32.0] - 2026-10-12

`ui`（[design.md 5.24](docs/design.md)）。G（表示とアプリ）の最初。

### 追加
- `import["std:ui"]`（`ui` feature。**既定では無効**で、`cargo build --features ui`）: `ui.run[view, options]` でウィンドウを開き、操作のたびに `view`（画面を返す関数）を呼び直す宣言的 UI。部品は `text` `button` `text_input` `checkbox` `slider` `select` `progress` `column` `row` `group` `scroll` `separator` `space`。ほかに `ui.refresh[]`（タイマーなどで状態を変えたときに画面を更新）と `ui.quit[]`
- GUI は egui（eframe、OpenGL）で、メインスレッドで動く。インタプリタは専用スレッドのままで、画面は `Send` できる木にして GUI に送り、操作はイベントループ（5.23）にメッセージで届く。部品の番号は画面の中の順番で決まるので、構造が同じ画面なら同じ部品に同じ番号がつく
- 日本語は、システムのフォント（游ゴシック・メイリオ・ヒラギノ・Noto Sans CJK・IPA など）を探して読み込む。`ui.run` の `font` でファイルも指定できる
- `examples/counter_ui.dol`
- 画面の木への変換とコールバックの往復を、GUI なしで確かめる単体テスト（描画は手で確かめる）

### 変更
- `dolphin` コマンドのメインスレッドが、インタプリタのスレッドが終わるまで、ウィンドウを開く依頼を待ち受けるようになった（`dolphin::ui::host`）

## [0.31.0] - 2026-10-12

非同期の関数（[design.md 5.23](docs/design.md)）。

### 追加
- イベントループ（`runtime/events.rs`）。補助スレッドで進めた処理の結果を、`Send` できるデータ（`runtime/data.rs` の `Data`）にしてインタプリタのスレッドのキューに届け、**コールバックを届いた順に 1 つずつ**呼ぶ。インタプリタは 1 スレッドのまま。スクリプトの最後に、待っているものがある間だけ自動で動く（REPL では動かさない）。コールバックのエラーは標準エラーに出して続ける
- `import["std:async"]`（標準ライブラリだけ）: `after[seconds, f]` `every[seconds, f]` `cancel[id]` `run[]` `stop[]`
- 非同期の関数（最後の引数がコールバック）: `http.request_async` `http.get_async` `http.post_async` `http.put_async` `http.delete_async` `os.run_async`。コールバックには、ブロッキング版が返す値（成功の値かエラー値）が 1 つ届く。戻り値は `cancel` に渡せる番号

### 変更
- `http.serve` は、このイベントループの上で動くようになった（タイマーや非同期の関数と一緒に使える。`async.stop[]` で戻る）。使い方は変わらない

## [0.30.0] - 2026-10-12

WebSocket（[design.md 5.22](docs/design.md)）。

### 追加
- `http.serve` のハンドラが `{"websocket": {"on_open": f, "on_message": f, "on_close": f}}` を返すと、その接続を WebSocket にする。コールバックは HTTP のハンドラと同じインタプリタのスレッドで 1 つずつ呼び（`on_message` はテキストなら文字列、バイナリならバイト列）、フレームの解析と送信は補助スレッドが行う。WebSocket の接続要求でないリクエストには `400` を返し、コールバックのエラーは標準エラーに出して続ける
- `import["std:ws"]`（標準ライブラリだけ）: `connect`（`ws://` のクライアント。`headers` と `timeout` を渡せる）`send` `recv` `close` `set_timeout`。サーバー側の接続のハンドルにも `send` `close` が使える。失敗はエラー値（種類 `timeout` `closed` `network` `protocol` `too_large` `parse`）
- RFC 6455 のフレーム（マスク、断片化、ping/pong、close、16 MB の上限、UTF-8 の検査）と、SHA-1・`Sec-WebSocket-Accept` の計算を自前で実装した（`stdlib/ws_proto.rs`）。`wss://` と permessage-deflate は未対応

## [0.29.0] - 2026-10-12

TCP（[design.md 5.21](docs/design.md)）。

### 追加
- 新しい値の型 `handle`（`type_of` は `"handle"`、表示は `<tcp 接続 127.0.0.1:80>`）。閉じる必要のある外部の資源を表し、参照されなくなると自動で閉じる
- `import["std:net"]`（標準ライブラリだけ、feature なし）: `connect` `listen` `accept` `read` `read_exact` `read_line` `write` `close` `set_timeout` `local_addr` `peer_addr`。ブロッキングで、失敗はエラー値（種類 `timeout` `eof` `closed` `parse` `network`）

## [0.28.0] - 2026-10-12

正規表現（[design.md 5.20](docs/design.md)）。F2 の最初。

### 追加
- `import["std:regex"]`（`regex` feature、既定で有効。外部クレート `regex` を使う）: `is_match` `find` `find_all` `replace` `replace_all` `split` `escape`
- `find` / `find_all` は一致マップ（`text` `start` `end` `groups` `named`）を返す。位置は文字単位。`replace` / `replace_all` の置き換えは文字列（`$1` `${name}`）か、一致マップを受け取る関数
- パターンの間違いはエラー値（種類 `parse`）。同じパターンは内部で使い回す

## [0.27.0] - 2026-10-12

ビット演算（[design.md 5.19](docs/design.md)）。整数型は作らない。

### 追加
- `import["std:bits"]`: `and` `or` `xor` `not` `shl` `shr` `popcount`。数値のうち ±2^53 以内の整数を、64 ビットの 2 の補数として計算する（`shr` は算術シフト）。整数でない引数、±2^53 を超える引数や結果、0〜63 でないシフトの量、負の数への `popcount` は実行時エラー

## [0.26.0] - 2026-10-12

分割代入（[design.md 5.19](docs/design.md)）。

### 追加
- 配列のパターン `{@a, @b, ...@rest} := 式`、マップのパターン `{"x": @x, "y": @y} := 式`、入れ子（`{"pos": {@px, @py}} := @entity`）。`:=` は宣言、`=` は宣言済みの変数への代入（`{@x, @y} = {@y, @x}` で入れ替えられる）
- 右辺の型が合わない、要素が足りない、キーがないときは実行時エラー。`...@rest` のないパターンは、余った要素があってもエラーにしない

## [0.25.0] - 2026-10-12

文字列の式展開と生の文字列（[design.md 5.19](docs/design.md)）。

### 追加
- 文字列の中の `@{ 式 }`: 式の値を文字列にして入れる（`"@a + @b = @{@a + @b}"`）。式の中に文字列・`{ }`・`@{ }` を入れ子にして書ける。式の中のエラーは、その位置を指す。文字としての `@{` は `\@{`（`@名前` は今までどおり）
- 生の文字列 `r"C:\path"`、`r#"引用 "ここ""#`（`#` を何個か挟むと `"` も含められる）。エスケープも展開もしない
- `dolphin::syntax::ast::StrSegment`、`lexer::string_end` `lexer::interpolation_end`

### 変更
- 構文木の文字列の部分が `StrPart` から `StrSegment`（文字か式）になった。`@名前` も式（変数）として扱う

## [0.24.0] - 2026-10-12

可変長引数と展開（[design.md 5.19](docs/design.md)）。

### 追加
- 可変長引数 `f[@a, ...@rest] ( … )`（`...@rest` は最後に書き、残りの引数の配列になる。既定値のある引数のあとでもよい）。無名関数も同じ。引数の数は「必須の数以上」で検査する
- 展開 `...式`: 呼び出しの引数（組み込み関数も）と配列リテラルの中で、配列の要素を並べる。配列でなければ実行時エラー。展開を含む呼び出しは、引数の数の検査が実行時になる。マップのリテラルには使えない
- 字句 `...`

## [0.23.0] - 2026-10-12

引数の既定値（[design.md 5.19](docs/design.md)）。

### 追加
- `f[@a, @b = 10] ( … )` のように、引数に既定値を書ける（名前つきの関数も無名関数も）。既定値のない引数より後ろにだけ書ける。渡されなかったときだけ、呼ぶたびに関数のスコープで評価し、前の引数を使える
- 引数の数は「必須の数以上、全部の数以下」で検査する（メッセージは `1 個以上 2 個以下です`）

### 変更
- 関数定義は、式として読む前に、文の先頭が `名前[…] (` の形かで見分けるようにした（引数に `=` を書けるようにするため）。`f[1] (` のような誤りのメッセージは「変数（`@x`）が必要です」になった

## [0.22.0] - 2026-10-12

条件式（[design.md 5.19](docs/design.md)）。

### 追加
- 式の中で `if 条件 ( 式 ) else ( 式 )`（`else if` も連ねられる）。`else` は省略できない。条件は真偽値で、選ばれなかった側は評価しない。文の先頭の `if` は今までどおり文

## [0.21.0] - 2026-10-12

`@m.key`（[design.md 5.19](docs/design.md)）。

### 追加
- マップ `@m` に対して `@m.key` は `@m{"key"}` と同じ。読み取りも代入もでき、連ねて書ける（`@cfg.db.port`、`@a.b{0}.c = 1`）。値が関数なら `@m.handler[1]` で呼べる。キーがなければ `{ }` の添字と同じ実行時エラー
- モジュールの `@u.name` は今までどおり。変数がモジュールかマップかは実行時に決まる

### 変更
- モジュールの変数への代入（`@u.x = 1`）の検査が、構文解析から実行時に移った（メッセージは同じ）。`.` の左がモジュールでもマップでもないときのエラーは「`.` を使えるのはモジュールかマップだけです」

## [0.20.0] - 2026-10-12

組み込み関数を隠す定義（[design.md 5.19](docs/design.md)）。

### 変更
- 組み込み関数と同じ名前の関数を、ファイルの中で定義できるようにした。そのファイルの中では定義した関数が優先され（関数値 `@f := str_len` も同じ）、ほかのファイルや組み込みモジュールには影響しない。それまでは「組み込み関数と同じ名前の関数は定義できません」というエラーだった。`import` だけは今までどおり定義できない

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
