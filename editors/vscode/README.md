# Dolphin の VS Code 拡張（シンタックスハイライト）

`.dol` ファイルに、色づけとコメント・括弧の設定をつけます。

## 入れ方

このフォルダ（`editors/vscode`）を、VS Code の拡張機能のフォルダにコピーして、VS Code を再起動します。

- Windows（PowerShell）: `Copy-Item -Recurse editors\vscode $env:USERPROFILE\.vscode\extensions\dolphin-lang`
- macOS / Linux: `cp -r editors/vscode ~/.vscode/extensions/dolphin-lang`

配布用に固めるなら、`npx @vscode/vsce package` で `.vsix` を作り、`code --install-extension dolphin-lang-0.1.0.vsix` で入れます。

## 色づけの対象

コメント、文字列（`\n` などのエスケープ、`@名前` と `@{ 式 }` の展開、`r"…"` の生の文字列）、数値（`0xFF` `1_000` `1e9`）、キーワード、`true` `false` `nil`、変数（`@x`）、関数の定義と呼び出し、`.名前`、`...`、演算子。

## 色の変えかた

変数（`@x`）の色は、名前が `#4EA1E8`、`@` が `#D19A66` です。変えたいときは、VS Code の `settings.json` に次のように書きます。

```json
"editor.tokenColorCustomizations": {
  "textMateRules": [
    { "scope": "variable.other.readwrite.dolphin", "settings": { "foreground": "#9CDCFE" } },
    { "scope": "punctuation.definition.variable.dolphin", "settings": { "foreground": "#FFB86C" } }
  ]
}
```
