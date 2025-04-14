# dolphin Language
🐬 自作インタプリタ言語 dolphin 🐬<br>
C++で実装されたインタプリタ<br>
拡張子 `.dol`<br>

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
`log[ Hello World!!! ]`標準出力<br>
`input[@var]`標準入力<br>
`window[@width, @height, @window_name(省略可)]`window生成<br>
- if文
```
if @a < @b {
    log[@a, より, @b, の方が大きいです!!!]
}
```
