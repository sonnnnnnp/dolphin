# Dolphin の VS Code 拡張（シンタックスハイライトと言語サーバー）

`.dol` ファイルに、色づけとコメント・括弧の設定をつけます。さらに `dolphin lsp`（言語サーバー）につないで、**定義へ移動**（F12）・**エラー表示**・**ホバー**・**アウトライン**・**整形**・**補完**・**参照の検索**・**リネーム**（F2）を使えるようにします（仕様は docs/design.md 5.33・5.34）。`dolphin` の実行ファイルが PATH にあれば、設定なしで動きます。PATH にないときは、設定 `dolphin.path` に実行ファイルの場所を書きます。

## 入れ方

Node.js が要ります。このフォルダ（`editors/vscode`）で、`.vsix` を作って入れます。

```
npm install
npm run package
code --install-extension dolphin-lang-0.2.0.vsix
```

試すだけなら、このフォルダを VS Code で開いて、`npm install` のあと F5（拡張機能の開発ホスト）でも動きます。

## ほかのエディタ

拡張は要りません。「`dolphin lsp` を言語サーバーとして起動する」設定を書きます（例: Neovim の `vim.lsp.start({ name = "dolphin", cmd = { "dolphin", "lsp" } })`、Helix の `languages.toml` の `[language-server.dolphin]` に `command = "dolphin"` と `args = ["lsp"]`）。

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
