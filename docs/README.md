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
if @a < @b {
    log[@a, より, @b, の方が大きいです!!!]
}

if @a > 0 && @b > 0 {
    log[両方正の数]
}
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

### ゲームループ
ウィンドウが閉じるまでブロック内を毎フレーム実行する（60fps）。
```
gameloop {
    // ここに毎フレームの処理を書く
}
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

gameloop {
    @vy = @vy + 1
    @py = @py + @vy

    if @py >= 420 {
        @py = 420
        @vy = 0
        @on_ground = 1
    }

    key_check[Space, @jump]
    key_check[Left,  @move_left]
    key_check[Right, @move_right]

    if @jump == 1 && @on_ground == 1 {
        @vy = -18
        @on_ground = 0
    }
    if @move_left == 1 {
        @px = @px - 5
    }
    if @move_right == 1 {
        @px = @px + 5
    }

    rect_set[player, @px, @py]
}
```
