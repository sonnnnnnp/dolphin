# dolphin Language
🐬 自作インタプリタ言語 dolphin 🐬
C++で実装されたインタプリタ
拡張子 `.dol`

## 構文
- コメントアウト
```
// ここはコメント
```
- 変数宣言
```
@foo = 123
@bar = 12 * 34
@baz = abc
```
- 組み込み関数
`log[]`標準出力
`input[]`標準入力
`window[@width, @height, @window_name]`window生成
- if文
```
if @a < @b {
    log[@a, より, @b, の方が大きいです!!!]
}
```
