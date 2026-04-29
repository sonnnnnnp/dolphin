# dolphin

C++ 実装のインタプリタ言語。拡張子は `.dol`。

---

## 言語仕様

### コメント

```
// ここはコメント
```

### 変数

`@` プレフィックスで宣言・参照する。数値・文字列どちらの値も格納でき、演算時に自動で数値として扱われる。

```
@x = 100
@y = 1.5
@name = dolphin
@result = @x * 2 + 10
```

### 演算子

| 種別 | 演算子 |
|------|--------|
| 算術 | `+` `-` `*` `/` `%` |
| 比較 | `==` `!=` `>` `<` `>=` `<=` |
| 論理 | `&&` `\|\|` |

```
@a = 10 % 3       // 1
@ok = @x > 0      // 比較結果は 1 または 0
```

### if / else

```
if @x > 0 (
    log[正の数]
)

if @x > 0 (
    log[正]
)
else (
    log[0 以下]
)

if @a > 0 && @b > 0 (
    log[両方正]
)
```

### while

```
@i = 0
while @i < 10 (
    log[@i]
    @i = @i + 1
)
```

### 配列

```
// 宣言
@nums  = {10, 20, 30, 40}
@words = {hello, world, foo}

// 読み取り
@x = @nums[0]          // 10
@y = @nums[0] + 5      // 15

// 書き換え
@nums[0] = 99
```

### 標準出力 `log`

`@変数名` をインライン展開して出力する。

```
log[こんにちは、@name！]
log[座標: (@x, @y)]
log[スコア: @score 点]
```

### ユーザー定義関数

`name[$param1, $param2] ( ... )` で関数を定義する。引数・戻り値どちらもサポート。

- 引数は `$` プレフィックスで受け取る（ローカル変数）
- `$var = expr` で関数内ローカル変数を宣言・更新
- `# expr` で値を返す（`return`）
- ローカル変数は関数外では参照できない

```
// 合計を返す関数
sum[$a, $b] (
    # $a + $b
)

@result = sum[10, 20]    // @result = 30
log[@result]
```

```
// 副作用だけの関数（戻り値なし）
greet[$name] (
    log[こんにちは、@name！]
)

greet[dolphin]
```

---

## 組み込み関数

### 汎用

| 関数 | 説明 |
|------|------|
| `input[@var]` | 標準入力を読み取り `@var` に格納 |
| `arr_len[arr, @var]` | 配列の長さを `@var` に格納 |
| `arr_set[arr, idx, val]` | `arr[idx]` に `val` をセット |
| `arr_push[arr, val]` | 配列末尾に `val` を追加 |
| `str_concat[@result, a, b]` | `a` と `b` を文字列結合して `@result` に格納 |
| `str_len[str, @result]` | 文字列の長さを `@result` に格納 |

```
arr_len[nums, @len]
arr_set[nums, 0, 99]
arr_push[nums, 100]
str_concat[@msg, Hello, World]   // @msg = "HelloWorld"
str_len[@msg, @n]                // @n = 10
```

### ウィンドウ

| 関数 | 説明 |
|------|------|
| `window[w, h, title]` | ウィンドウを作成（60fps）|
| `bg[r, g, b]` | 背景色を設定 |
| `gameloop ( ... )` | ウィンドウが閉じるまで毎フレーム実行 |
| `camera_set[x, y]` | ビュー左上座標を指定 |

```
window[800, 600, My Game]
bg[100, 149, 237]

gameloop (
    // 毎フレームの処理
)
```

### 矩形

| 関数 | 説明 |
|------|------|
| `rect_create[id, x, y, w, h, r, g, b]` | 矩形を作成・登録（永続） |
| `rect_set[id, x, y]` | 矩形の位置を更新 |
| `rect_draw[x, y, w, h, r, g, b]` | そのフレームだけ矩形を描画（次フレームで自動消去） |

描画順は `rect_create` を呼んだ順（先が下のレイヤー）。

```
rect_create[player, 100, 400, 40, 60, 255, 0, 0]
rect_set[player, @px, @py]

// 一時的な矩形（毎フレーム呼ぶ）
rect_draw[@px, @py, 40, 60, 255, 0, 0]
```

### 円

| 関数 | 説明 |
|------|------|
| `circle_create[id, x, y, radius, r, g, b]` | 円を作成・登録 |
| `circle_set[id, x, y]` | 円の位置を更新 |

```
circle_create[ball, 200, 200, 20, 255, 255, 0]
circle_set[ball, @bx, @by]
```

### 画像（スプライト）

| 関数 | 説明 |
|------|------|
| `img_load[id, path]` | 画像を読み込む |
| `img_draw[id, x, y]` | 座標に描画（gameloop 内で毎フレーム呼ぶ）|
| `img_flip[id, 1\|0]` | 左右反転（1=反転, 0=通常）|

```
img_load[player, assets/player.png]
img_flip[player, @facing]
img_draw[player, @px, @py]
```

### 描画レイヤー順（下から上）

1. `rect_create` で登録した矩形
2. `circle_create` で登録した円
3. `rect_draw` で一時描画した矩形
4. `img_draw` で描画したスプライト
5. `text_create` で登録したテキスト（最前面）

---

### テキスト

| 関数 | 説明 |
|------|------|
| `font_load[id, path]` | フォントを読み込む |
| `text_create[id, font, x, y, str, size, r, g, b]` | テキストを作成・登録 |
| `text_set[id, x, y]` | テキストの位置を更新 |
| `text_set_str[id, str]` | テキストの文字列を更新（`@var` 展開あり）|

```
font_load[font, /System/Library/Fonts/Helvetica.ttc]
text_create[score_lbl, font, 10, 10, Score: 0, 22, 255, 255, 100]
text_set[score_lbl, @cam_x + 10, 10]
text_set_str[score_lbl, Score: @score]
```

### キー入力

押下中なら `1`、離していれば `0` を変数にセット。

| 関数 | 説明 |
|------|------|
| `key_check[key, @var]` | キー状態を `@var` に格納 |

対応キー: `Left` `Right` `Up` `Down` `Space` `Enter` `A` `D` `W` `S` `X` `Z`

```
key_check[Left,  @go_left]
key_check[Right, @go_right]
key_check[Space, @jump]
```

### マウス入力

| 関数 | 説明 |
|------|------|
| `mouse_pos[@x, @y]` | マウスのワールド座標を取得 |
| `mouse_click[@var]` | そのフレームに左クリックされたら `1` |

```
mouse_pos[@mx, @my]
mouse_click[@clicked]
```

### サウンド

| 関数 | 説明 |
|------|------|
| `sound_load[id, path]` | 音声ファイルを読み込む（.wav）|
| `sound_play[id]` | 再生 |
| `sound_vol[id, vol]` | 音量設定（0〜100）|

```
sound_load[coin_se, assets/coin.wav]
sound_play[coin_se]
sound_vol[coin_se, 80]
```

---

## サンプル

### BMI 計算 (`my_scripts/bmi.dol`)

```
log[身長を入力してください(cm)]
input[@height]
log[体重を入力してください(kg)]
input[@weight]

@bmi = @weight * 10000 / (@height * @height)

@msg = 痩せすぎです
if @bmi > 18 ( @msg = 普通体重です )
if @bmi > 25 ( @msg = 肥満(1度)です )

log[BMI: @bmi  @msg]
```

### Pong (`my_scripts/pong.dol`)

左プレイヤー W/S・右プレイヤー Up/Down で操作。ボールのヒット位置で反射角が変わる。

```
window[800, 600, Pong]
bg[15, 15, 35]
font_load[f, /System/Library/Fonts/Helvetica.ttc]

@lp_y = 260
@rp_y = 260
@bx   = 394
@by   = 294
@bvx  = 5
@bvy  = 3
@sl   = 0
@sr   = 0

rect_create[cline, 396, 0,   4, 600, 50, 50, 80]
rect_create[lp,   20,  260, 12, 80,  255, 255, 255]
rect_create[rp,   768, 260, 12, 80,  255, 255, 255]
rect_create[ball, 394, 294, 12, 12,  255, 220, 80]
text_create[tl, f, 300, 20, 0, 64, 255, 255, 255]
text_create[tr, f, 460, 20, 0, 64, 255, 255, 255]

gameloop (
    // パドル操作・ボール移動・得点判定 ...
    rect_set[lp,   20,  @lp_y]
    rect_set[rp,   768, @rp_y]
    rect_set[ball, @bx, @by]
    text_set_str[tl, @sl]
    text_set_str[tr, @sr]
)
```

### ブロック崩し (`my_scripts/breakout.dol`)

50ブロック（10列×5行）、ライフ制、スピードアップ、タイトル/ゲームオーバー/クリア画面あり。`rect_draw[]` を使って毎フレームブロックを描画している。

### 横スクロールマリオ (`my_scripts/mario.dol`)

左右矢印で移動、Space でジャンプ。足場・コイン・敵・カメラ追従あり。

```
window[800, 600, dolphin Mario]
bg[100, 149, 237]
img_load[player, assets/player.png]

@px = 80
@py = 472
@vy = 0

gameloop (
    key_check[Left,  @go_left]
    key_check[Right, @go_right]
    key_check[Space, @jump]

    if @go_left  == 1 ( @px = @px - 5 )
    if @go_right == 1 ( @px = @px + 5 )

    @vy = @vy + 1
    @py = @py + @vy
    if @py >= 472 ( @py = 472  @vy = 0 )
    if @jump == 1 && @vy == 0 ( @vy = -18 )

    @cam_x = @px - 400
    if @cam_x < 0 ( @cam_x = 0 )
    camera_set[@cam_x, 0]

    img_draw[player, @px, @py]
)
```
