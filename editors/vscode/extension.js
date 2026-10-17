// `dolphin lsp` を言語サーバーとして起動する（design.md 5.33）
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate() {
  const command = vscode.workspace.getConfiguration("dolphin").get("path") || "dolphin";
  client = new LanguageClient(
    "dolphin",
    "Dolphin",
    { command, args: ["lsp"] },
    { documentSelector: [{ scheme: "file", language: "dolphin" }] }
  );
  client.start().catch((e) => {
    vscode.window.showErrorMessage(
      `dolphin lsp を起動できませんでした（設定 dolphin.path: ${command}）: ${e.message}`
    );
  });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
