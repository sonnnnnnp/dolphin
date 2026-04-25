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
```
@foo = 123
@bar = 12 * 34
@baz = abc
```

### 演算子
```
@a = 10 + 3    // 加算
@b = 10 - 3    // 減算
@c = 10 * 3    // 乗算
@d = 10 / 3    // 除算
```

### 組み込み関数
標準出力（変数・複数引数可）
```
log[ Hello World!!! ]
```

標準入力
```
input[@var]
```

### if文
比較演算子: `>` `<` `>=` `<=` `==` `!=`
論理演算子: `&&` `||`
```
if @a < @b (
    log[@a, より, @b, の方が大きいです!!!]
)

if @a > 0 && @b > 0 (
    log[両方正の数]
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
rect_set[player, @px, @py]
```
描画順は `rect_create` を呼んだ順番（先に作ったものが下のレイヤー）。

### 背景色
```
// bg[r, g, b]
bg[100, 149, 237]
```

### 配列

#### 宣言
```
@arr = {10, 20, 30, hoge}
```

#### 読み取り（式の中で使用可）
```
@x = @arr[0]        // 10
@y = @arr[0] + 5    // 15
```

#### 要素の書き換え
```
@arr[0] = 99
arr_set[arr, 0, 99]   // 同じ意味（@なしで配列名を渡す）
```

#### 組み込み関数
```
arr_len[arr, @len]       // 長さを @len に格納
arr_set[arr, 1, hello]   // インデックス1に値をセット
arr_push[arr, newval]    // 末尾に追加
```

#### while と組み合わせたループ
```
@i = 0
while @i < @len (
    log[@arr[@i]]
    @i = @i + 1
)
```

---

### キー入力
`key_check` は押下中なら `@var = 1`、離していれば `@var = 0` をセットする。
```
key_check[Space, @jump]
key_check[Left,  @go_left]
key_check[Right, @go_right]
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

### スプライト（画像）

```
img_load[player, assets/player.png]   // 画像を読み込む
img_draw[player, @x, @y]             // 座標に描画（gameloop内で毎フレーム呼ぶ）
img_flip[player, 1]                   // 左右反転 (1=反転, 0=通常)
```

描画順は `img_draw` を呼んだ順番。`rect_create` で作った矩形より前面に描画される。

### カメラ

```
camera_set[@cam_x, @cam_y]
```

ビューの左上座標を指定する。全描画（矩形・スプライト）にカメラオフセットが適用される。

プレイヤー追従の例:
```
@cam_x = @px - 400
if @cam_x < 0 (
    @cam_x = 0
)
camera_set[@cam_x, 0]
```

### ゲームループ
ウィンドウが閉じるまでブロック内を毎フレーム実行する（60fps）。
```
gameloop (
    // ここに毎フレームの処理を書く
)
```

---

## ゲームサンプル
`my_scripts/mario.dol` — 左右矢印で移動、Spaceでジャンプ、足場あり。

```
window[800, 600, dolphin Mario]

rect_create[sky,    0,   0, 800, 480, 100, 149, 237]
rect_create[ground, 0, 480, 800, 120,  80,  50,  20]
rect_create[player, 100, 420, 40, 60, 220, 50, 50]

@px = 100
@py = 420
@vy = 0
@on_ground = 0

gameloop (
    @vy = @vy + 1
    @py = @py + @vy

    if @py >= 420 (
        @py = 420
        @vy = 0
        @on_ground = 1
    )

    key_check[Space, @jump]
    key_check[Left,  @move_left]
    key_check[Right, @move_right]

    if @jump == 1 && @on_ground == 1 (
        @vy = -18
        @on_ground = 0
    )
    if @move_left == 1 (
        @px = @px - 5
    )
    if @move_right == 1 (
        @px = @px + 5
    )

    rect_set[player, @px, @py]
)
```
