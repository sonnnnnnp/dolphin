# dolphin Language
🐬 自作インタプリタ言語 dolphin 🐬<br>
C++で実装されたインタプリタ<br>
拡張子は `.dol`<br>

## 構文

### コメントアウト
```
// ここはコメント
```

### 変数宣言

`$` は数値変数、`@` は文字列変数。

```
$x = 100
$y = 12 * 34

@name = dolphin
@msg = こんにちは
```

### 演算子
```
$a = 10 + 3    // 加算
$b = 10 - 3    // 減算
$c = 10 * 3    // 乗算
$d = 10 / 3    // 除算（整数）
```

### 標準出力

`log[]` は引数1つ。`$変数名` / `@変数名` をインライン展開して出力する。

```
log[こんにちは、@name！]
log[スコア: $score 点]
log[座標: ($x, $y)]
```

### 標準入力
```
input[$num]    // 数値として受け取る
input[@str]    // 文字列として受け取る
```

### if 文
比較演算子: `>` `<` `>=` `<=` `==` `!=`
論理演算子: `&&` `||`
```
if $a < $b (
    log[$b の方が大きい]
)

if $a > 0 && $b > 0 (
    log[両方正の数]
)

if @name == dolphin (
    log[名前が一致した]
)
```

### while ループ
```
$i = 0
while $i < 10 (
    log[$i]
    $i = $i + 1
)
```

---

## 配列

### 宣言

```
$nums  = {10, 20, 30, 40}    // 数値配列
@words = {hello, world, foo} // 文字列配列
```

### 読み取り（式の中で使用可）
```
$x = $nums[0]        // 10
$y = $nums[0] + 5    // 15
log[@words[1]]       // "world" を出力
```

### 要素の書き換え
```
$nums[0] = 99
arr_set[nums, 0, 99]   // 同じ意味（配列名はシジルなしで渡す）
```

### 組み込み関数
```
arr_len[nums,  $len]      // 長さを $len に格納
arr_set[nums,  1, 99]     // インデックス1に値をセット
arr_push[nums, 100]       // 末尾に追加
```

### while と組み合わせたループ
```
arr_len[nums, $len]
$i = 0
while $i < $len (
    log[$nums[$i]]
    $i = $i + 1
)
```

---

## ゲーム機能 (SFML)

### ウィンドウ作成
`window[]` はノンブロッキング。呼び出し後もスクリプトの実行が続く。
```
window[800, 600, タイトル]
```

### 矩形エンティティ
```
// rect_create[id, x, y, width, height, r, g, b]
rect_create[player, 100, 400, 40, 60, 255, 0, 0]

// rect_set[id, x, y]  位置を更新
rect_set[player, $px, $py]
```
描画順は `rect_create` を呼んだ順番（先に作ったものが下のレイヤー）。

### 背景色
```
bg[100, 149, 237]
```

### スプライト（画像）
```
img_load[player, assets/player.png]    // 画像を読み込む
img_draw[player, $x, $y]              // 座標に描画（gameloop 内で毎フレーム呼ぶ）
img_flip[player, 1]                    // 左右反転 (1=反転, 0=通常)
```
描画順は `img_draw` を呼んだ順番。`rect_create` で作った矩形より前面に描画される。

### キー入力
`key_check` は押下中なら `$var = 1`、離していれば `$var = 0` をセットする。
```
key_check[Space, $jump]
key_check[Left,  $go_left]
key_check[Right, $go_right]
```

対応キー一覧:
| キー名 | キー |
|--------|------|
| `Left` | ← |
| `Right` | → |
| `Up` | ↑ |
| `Down` | ↓ |
| `Space` | スペース |
| `Enter` | エンター |
| `Z` `X` `A` `D` `W` `S` | 各アルファベット |

### カメラ
```
camera_set[$cam_x, $cam_y]
```
ビューの左上座標を指定する。全描画（矩形・スプライト）にカメラオフセットが適用される。

プレイヤー追従の例:
```
$cam_x = $px - 400
if $cam_x < 0 (
    $cam_x = 0
)
camera_set[$cam_x, 0]
```

### ゲームループ
ウィンドウが閉じるまでブロック内を毎フレーム実行する（60fps）。
```
gameloop (
    // ここに毎フレームの処理を書く
)
```

---

## サンプル

### BMI計算 (`my_scripts/bmi.dol`)
```
$height_cm = 0
$weight_kg = 0

log[身長を入力してください(cm)]
input[$height_cm]
log[体重を入力してください(kg)]
input[$weight_kg]

$bmi = $weight_kg * 10000
$bmi = $bmi / ($height_cm * $height_cm)

@msg = 痩せすぎです
if $bmi > 18 (
    @msg = 普通体重です
)
if $bmi > 25 (
    @msg = 肥満です
)

log[BMI: $bmi  @msg]
```

### 横スクロールマリオ (`my_scripts/mario.dol`)
左右矢印で移動、Space でジャンプ。足場・コインあり、カメラ追従。
```
window[800, 600, dolphin Mario]
bg[100, 149, 237]

img_load[player, assets/player.png]

$plat_x = {160, 380, 560}
$plat_y = {440, 390, 450}
$plat_w = {3, 4, 3}

$px = 80
$py = 472
$vy = 0

arr_len[plat_x, $plen]

gameloop (
    key_check[Left,  $go_left]
    key_check[Right, $go_right]
    key_check[Space, $jump]

    if $go_left  == 1 ( $px = $px - 5 )
    if $go_right == 1 ( $px = $px + 5 )

    $vy = $vy + 1
    $py = $py + $vy

    $on_ground = 0
    if $py >= 472 (
        $py = 472
        $vy = 0
        $on_ground = 1
    )

    if $jump == 1 && $on_ground == 1 (
        $vy = -18
    )

    camera_set[$px - 400, 0]
    img_draw[player, $px, $py]
)
```
