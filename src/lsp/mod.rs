//! 言語サーバー（`dolphin lsp`、design.md 5.33）。標準入出力で LSP（JSON-RPC）を話す

pub mod analysis;
mod completion;
pub mod json;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use analysis::{Analysis, Def, DefKind, Target, analyze};
use json::Json;

use crate::diag::Diagnostic;

/// `input` から要求を読み、`output` へ応答を書く。`exit` 通知で終わり、終了コードを返す
/// （`shutdown` の後なら 0、なければ 1）
pub fn serve(input: &mut dyn BufRead, output: &mut dyn Write) -> i32 {
    let mut sink = Vec::new();
    let catalog = crate::Session::new(&mut sink).catalog();
    let mut server = Server {
        docs: HashMap::new(),
        fresh: HashMap::new(),
        last_good: HashMap::new(),
        catalog,
        shutdown: false,
    };
    loop {
        let message = match read_message(input) {
            Ok(Some(message)) => message,
            Ok(None) | Err(_) => return if server.shutdown { 0 } else { 1 },
        };
        let Ok(message) = Json::parse(&message) else {
            continue;
        };
        let method = message
            .get("method")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Json::Null);
        let id = message.get("id").cloned();
        if method == "exit" {
            return if server.shutdown { 0 } else { 1 };
        }
        let mut replies = Vec::new();
        match id {
            Some(id) => {
                let reply = match server.request(&method, &params) {
                    Ok(result) => Json::obj([
                        ("jsonrpc", Json::str("2.0")),
                        ("id", id),
                        ("result", result),
                    ]),
                    Err((code, text)) => Json::obj([
                        ("jsonrpc", Json::str("2.0")),
                        ("id", id),
                        (
                            "error",
                            Json::obj([
                                ("code", Json::Num(code as f64)),
                                ("message", Json::str(text)),
                            ]),
                        ),
                    ]),
                };
                replies.push(reply);
            }
            None => replies.extend(server.notification(&method, &params)),
        }
        for reply in replies {
            if write_message(output, &reply).is_err() {
                return 1;
            }
        }
    }
}

fn read_message(input: &mut dyn BufRead) -> io::Result<Option<String>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Content-Length がありません",
        ));
    };
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_message(output: &mut dyn Write, message: &Json) -> io::Result<()> {
    let body = message.to_string();
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    output.flush()
}

const METHOD_NOT_FOUND: i32 = -32601;
const REQUEST_FAILED: i32 = -32803;
const CANNOT_RENAME: &str =
    "この名前はリネームできません（組み込み、ほかのファイルの宣言、または宣言が見つかりません）";

struct Server {
    /// 開いている文書（URI → 内容）
    docs: HashMap<String, String>,
    /// 現在の内容を解析できたときの結果
    fresh: HashMap<String, Rc<Analysis>>,
    /// 直前に解析できた結果（補完は、構文エラーの間もこれを使う）
    last_good: HashMap<String, Rc<Analysis>>,
    catalog: crate::Catalog,
    shutdown: bool,
}

type Reply = Result<Json, (i32, String)>;

impl Server {
    fn request(&mut self, method: &str, params: &Json) -> Reply {
        match method {
            "initialize" => Ok(Json::obj([
                (
                    "capabilities",
                    Json::obj([
                        ("textDocumentSync", Json::num(1)),
                        ("definitionProvider", Json::Bool(true)),
                        ("hoverProvider", Json::Bool(true)),
                        ("documentSymbolProvider", Json::Bool(true)),
                        ("documentFormattingProvider", Json::Bool(true)),
                        ("referencesProvider", Json::Bool(true)),
                        (
                            "renameProvider",
                            Json::obj([("prepareProvider", Json::Bool(true))]),
                        ),
                        (
                            "completionProvider",
                            Json::obj([(
                                "triggerCharacters",
                                Json::Arr(vec![Json::str("@"), Json::str("."), Json::str(":")]),
                            )]),
                        ),
                    ]),
                ),
                (
                    "serverInfo",
                    Json::obj([
                        ("name", Json::str("dolphin")),
                        ("version", Json::str(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
            ])),
            "shutdown" => {
                self.shutdown = true;
                Ok(Json::Null)
            }
            "textDocument/definition" => Ok(self.definition(params).unwrap_or(Json::Null)),
            "textDocument/hover" => Ok(self.hover(params).unwrap_or(Json::Null)),
            "textDocument/documentSymbol" => {
                Ok(self.document_symbols(params).unwrap_or(Json::Arr(vec![])))
            }
            "textDocument/completion" => Ok(self.completion(params).unwrap_or(Json::Arr(vec![]))),
            "textDocument/references" => Ok(self.references(params).unwrap_or(Json::Arr(vec![]))),
            "textDocument/prepareRename" => self.prepare_rename(params),
            "textDocument/rename" => self.rename(params),
            "textDocument/formatting" => Ok(self.formatting(params).unwrap_or(Json::Null)),
            _ => Err((METHOD_NOT_FOUND, format!("未対応の要求です: {method}"))),
        }
    }

    /// 通知を処理して、送り返す通知を返す
    fn notification(&mut self, method: &str, params: &Json) -> Vec<Json> {
        let uri = params
            .path("textDocument.uri")
            .and_then(Json::as_str)
            .map(str::to_string);
        match (method, uri) {
            ("textDocument/didOpen", Some(uri)) => {
                let text = params
                    .path("textDocument.text")
                    .and_then(Json::as_str)
                    .unwrap_or("");
                self.set_doc(&uri, text);
                vec![self.diagnostics(&uri)]
            }
            ("textDocument/didChange", Some(uri)) => {
                // 全文を送る方式。最後の変更が最新
                let text = match params.get("contentChanges") {
                    Some(Json::Arr(changes)) => changes
                        .last()
                        .and_then(|c| c.get("text"))
                        .and_then(Json::as_str),
                    _ => None,
                };
                match text {
                    Some(text) => {
                        self.set_doc(&uri, text);
                        vec![self.diagnostics(&uri)]
                    }
                    None => vec![],
                }
            }
            ("textDocument/didSave", Some(uri)) if self.docs.contains_key(&uri) => {
                vec![self.diagnostics(&uri)]
            }
            ("textDocument/didClose", Some(uri)) => {
                self.docs.remove(&uri);
                self.fresh.remove(&uri);
                self.last_good.remove(&uri);
                vec![publish(&uri, vec![])]
            }
            _ => vec![],
        }
    }

    fn set_doc(&mut self, uri: &str, text: &str) {
        self.docs.insert(uri.to_string(), text.to_string());
        match analyze(text) {
            Ok(analysis) => {
                let analysis = Rc::new(analysis);
                self.fresh.insert(uri.to_string(), analysis.clone());
                self.last_good.insert(uri.to_string(), analysis);
            }
            Err(_) => {
                self.fresh.remove(uri);
            }
        }
    }

    // ---- 診断 ----

    fn diagnostics(&self, uri: &str) -> Json {
        let src = &self.docs[uri];
        let dir = uri_to_path(uri)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let mut sink = Vec::new();
        let result = crate::Session::with_base_dir(&mut sink, dir).check(src);
        let list = match result {
            Ok(()) => vec![],
            Err(diag) => vec![diagnostic_json(src, &diag)],
        };
        publish(uri, list)
    }

    // ---- 位置から名前を引く ----

    fn text_and_analysis(&self, params: &Json) -> Option<(String, &str, Rc<Analysis>)> {
        let uri = params.path("textDocument.uri")?.as_str()?.to_string();
        let src = self.docs.get(&uri).map(String::as_str)?;
        let analysis = self.fresh.get(&uri)?.clone();
        Some((uri, src, analysis))
    }

    /// カーソルの 1 始まりの（行, 文字の列）と、その行の文字列
    fn cursor<'a>(&self, params: &Json, src: &'a str) -> Option<(usize, usize, &'a str)> {
        let line0 = params.path("position.line")?.as_usize()?;
        let units = params.path("position.character")?.as_usize()?;
        let text = src.lines().nth(line0).unwrap_or("");
        Some((line0 + 1, utf16_to_col(text, units), text))
    }

    fn completion(&self, params: &Json) -> Option<Json> {
        let uri = params.path("textDocument.uri")?.as_str()?;
        let src = self.docs.get(uri)?;
        let analysis = self.last_good.get(uri)?;
        let (line, col, text) = self.cursor(params, src)?;
        let before: String = text.chars().take(col - 1).collect();
        let dir = uri_to_path(uri).and_then(|p| p.parent().map(Path::to_path_buf));
        Some(completion::complete(
            analysis,
            &self.catalog,
            dir.as_deref(),
            &before,
            line,
            text,
        ))
    }

    /// カーソルの名前の宣言（このファイルの中のものだけ）
    fn local_def_at(&self, params: &Json) -> Option<(String, &str, Rc<Analysis>, Rc<Def>)> {
        let (uri, src, analysis) = self.text_and_analysis(params)?;
        let (line, col, _) = self.cursor(params, src)?;
        let Target::Def(def) = &analysis.ref_at(line, col)?.target else {
            return None;
        };
        let def = def.clone();
        Some((uri, src, analysis, def))
    }

    fn references(&self, params: &Json) -> Option<Json> {
        let (uri, src, analysis, def) = self.local_def_at(params)?;
        let include_decl = !matches!(
            params.path("context.includeDeclaration"),
            Some(Json::Bool(false))
        );
        let list = analysis
            .refs_to(&def)
            .filter(|r| include_decl || r.span != def.span)
            .map(|r| {
                Json::obj([
                    ("uri", Json::str(&uri)),
                    ("range", range_json(src, r.span.line, r.span.col, r.len)),
                ])
            })
            .collect();
        Some(Json::Arr(list))
    }

    fn prepare_rename(&self, params: &Json) -> Reply {
        let Some((_, src, analysis, def)) = self.local_def_at(params) else {
            return Err((REQUEST_FAILED, CANNOT_RENAME.to_string()));
        };
        let (line, col, _) = self
            .cursor(params, src)
            .ok_or((REQUEST_FAILED, CANNOT_RENAME.to_string()))?;
        let r = analysis
            .ref_at(line, col)
            .ok_or((REQUEST_FAILED, CANNOT_RENAME.to_string()))?;
        Ok(Json::obj([
            ("range", range_json(src, r.span.line, r.span.col, r.len)),
            ("placeholder", Json::str(def.name.clone())),
        ]))
    }

    fn rename(&self, params: &Json) -> Reply {
        let fail = |text: &str| Err((REQUEST_FAILED, text.to_string()));
        let Some((uri, src, analysis, def)) = self.local_def_at(params) else {
            return fail(CANNOT_RENAME);
        };
        let raw = params.get("newName").and_then(Json::as_str).unwrap_or("");
        let new = raw.strip_prefix('@').unwrap_or(raw);
        let valid = new
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && new.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return fail("名前は英数字と `_` で、数字で始められません");
        }
        if analysis
            .defs
            .iter()
            .any(|d| d.kind == def.kind && d.name == new && !Rc::ptr_eq(d, &def))
        {
            return fail(&format!(
                "同じ種類の `{new}` がこのファイルにすでにあります"
            ));
        }
        let text = if def.kind == DefKind::Var {
            format!("@{new}")
        } else {
            new.to_string()
        };
        let edits = analysis
            .refs_to(&def)
            .map(|r| {
                Json::obj([
                    ("range", range_json(src, r.span.line, r.span.col, r.len)),
                    ("newText", Json::str(text.clone())),
                ])
            })
            .collect();
        Ok(Json::obj([(
            "changes",
            Json::obj([(uri.as_str(), Json::Arr(edits))]),
        )]))
    }

    /// カーソル位置にある名前の宣言と、その宣言があるファイル（このファイルなら None）
    fn target_at(&self, params: &Json) -> Option<(Rc<Def>, Option<PathBuf>, String)> {
        let (uri, src, analysis) = self.text_and_analysis(params)?;
        let line0 = params.path("position.line")?.as_usize()?;
        let units = params.path("position.character")?.as_usize()?;
        let col = utf16_to_col(src.lines().nth(line0).unwrap_or(""), units);
        let r = analysis.ref_at(line0 + 1, col)?;
        match &r.target {
            Target::Def(def) => Some((def.clone(), None, uri)),
            Target::Member { path, name } => {
                let dir = uri_to_path(&uri)?.parent()?.to_path_buf();
                let file = crate::runtime::module::resolve_import(&dir, path).ok()?;
                let text = std::fs::read_to_string(&file).ok()?;
                let def = analyze(&text).ok()?.exports.get(name)?.clone();
                Some((def, Some(file), uri))
            }
        }
    }

    fn definition(&self, params: &Json) -> Option<Json> {
        let (def, file, uri) = self.target_at(params)?;
        let (uri, text) = match file {
            Some(file) => {
                let text = std::fs::read_to_string(&file).ok()?;
                (path_to_uri(&file), text)
            }
            None => (uri.clone(), self.docs.get(&uri)?.clone()),
        };
        Some(Json::obj([
            ("uri", Json::str(uri)),
            (
                "range",
                range_json(&text, def.span.line, def.span.col, def.len),
            ),
        ]))
    }

    fn hover(&self, params: &Json) -> Option<Json> {
        let (def, _, _) = self.target_at(params)?;
        let value = match def.hover.split_once("\n\n") {
            Some((code, doc)) => format!("```dolphin\n{code}\n```\n\n{doc}"),
            None => format!("```dolphin\n{}\n```", def.hover),
        };
        Some(Json::obj([(
            "contents",
            Json::obj([("kind", Json::str("markdown")), ("value", Json::Str(value))]),
        )]))
    }

    fn document_symbols(&self, params: &Json) -> Option<Json> {
        let (_, src, analysis) = self.text_and_analysis(params)?;
        let lines: Vec<&str> = src.lines().collect();
        let symbols = analysis
            .symbols
            .iter()
            .map(|s| {
                let last = lines.get(s.last_line - 1).copied().unwrap_or("");
                let range = Json::obj([
                    ("start", position(s.first_line - 1, 0)),
                    (
                        "end",
                        position(s.last_line - 1, last.encode_utf16().count()),
                    ),
                ]);
                let (name, kind) = match s.kind {
                    DefKind::Func => (s.name.clone(), 12),
                    DefKind::Var => (format!("@{}", s.name), 13),
                };
                Json::obj([
                    ("name", Json::Str(name)),
                    ("kind", Json::num(kind)),
                    ("range", range),
                    (
                        "selectionRange",
                        range_json(src, s.span.line, s.span.col, s.len),
                    ),
                ])
            })
            .collect();
        Some(Json::Arr(symbols))
    }

    fn formatting(&self, params: &Json) -> Option<Json> {
        let uri = params.path("textDocument.uri")?.as_str()?;
        let src = self.docs.get(uri)?;
        let formatted = crate::fmt::format_source(src).ok()?;
        if &formatted == src {
            return Some(Json::Arr(vec![]));
        }
        let end_line = src.split('\n').count() - 1;
        let end_char = src
            .split('\n')
            .next_back()
            .map_or(0, |l| l.encode_utf16().count());
        Some(Json::Arr(vec![Json::obj([
            (
                "range",
                Json::obj([
                    ("start", position(0, 0)),
                    ("end", position(end_line, end_char)),
                ]),
            ),
            ("newText", Json::Str(formatted)),
        ])]))
    }
}

// ---- JSON の組み立て ----

fn publish(uri: &str, diagnostics: Vec<Json>) -> Json {
    Json::obj([
        ("jsonrpc", Json::str("2.0")),
        ("method", Json::str("textDocument/publishDiagnostics")),
        (
            "params",
            Json::obj([
                ("uri", Json::str(uri)),
                ("diagnostics", Json::Arr(diagnostics)),
            ]),
        ),
    ])
}

fn diagnostic_json(src: &str, diag: &Diagnostic) -> Json {
    // import したファイルで起きたエラーは、このファイルの位置を持たないので先頭に出す
    let (line, col, len) = if diag.origin.is_some() {
        (1, 1, 0)
    } else {
        let text = src.lines().nth(diag.span.line - 1).unwrap_or("");
        (diag.span.line, diag.span.col, word_len(text, diag.span.col))
    };
    let mut message = diag.message.clone();
    for note in &diag.notes {
        message += &format!("\n{note}");
    }
    Json::obj([
        ("range", range_json(src, line, col, len.max(1))),
        ("severity", Json::num(1)),
        ("source", Json::str("dolphin")),
        ("message", Json::Str(message)),
    ])
}

/// `col`（1 始まり）から始まる単語（`@名前` や名前）の文字数。単語でなければ 1
fn word_len(line: &str, col: usize) -> usize {
    let rest: Vec<char> = line.chars().skip(col - 1).collect();
    let skip = usize::from(rest.first() == Some(&'@'));
    let n = rest[skip..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
        .count();
    if n == 0 { 1 } else { n + skip }
}

fn position(line0: usize, units: usize) -> Json {
    Json::obj([("line", Json::num(line0)), ("character", Json::num(units))])
}

/// 1 始まりの行・文字から `len` 文字の範囲
fn range_json(src: &str, line: usize, col: usize, len: usize) -> Json {
    let text = src.lines().nth(line.saturating_sub(1)).unwrap_or("");
    let units = |upto: usize| text.chars().take(upto).map(char::len_utf16).sum::<usize>();
    Json::obj([
        ("start", position(line - 1, units(col - 1))),
        ("end", position(line - 1, units(col - 1 + len))),
    ])
}

/// LSP の UTF-16 の位置を、1 始まりの文字の列にする
fn utf16_to_col(line: &str, units: usize) -> usize {
    let mut used = 0;
    for (i, c) in line.chars().enumerate() {
        if used >= units {
            return i + 1;
        }
        used += c.len_utf16();
    }
    line.chars().count() + 1
}

// ---- URI ----

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hex = std::str::from_utf8(&raw[i + 1..i + 3]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    // Windows: `/c:/dir/file` → `c:/dir/file`
    let b = path.as_bytes();
    let path = if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        path[1..].to_string()
    } else {
        path
    };
    Some(PathBuf::from(path))
}

fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~' | b':') {
            uri.push(b as char);
        } else {
            uri += &format!("%{b:02X}");
        }
    }
    uri
}

/// 標準入出力で動かす
pub fn run_stdio() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(&mut stdin.lock(), &mut stdout.lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &Json) -> String {
        let body = json.to_string();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    fn call(id: usize, method: &str, params: Json) -> String {
        frame(&Json::obj([
            ("jsonrpc", Json::str("2.0")),
            ("id", Json::num(id)),
            ("method", Json::str(method)),
            ("params", params),
        ]))
    }

    fn notify(method: &str, params: Json) -> String {
        frame(&Json::obj([
            ("jsonrpc", Json::str("2.0")),
            ("method", Json::str(method)),
            ("params", params),
        ]))
    }

    fn at(uri: &str, line: usize, character: usize) -> Json {
        Json::obj([
            ("textDocument", Json::obj([("uri", Json::str(uri))])),
            ("position", position(line, character)),
        ])
    }

    /// 送った要求に対する出力を、メッセージごとに取り出す
    fn run(input: String) -> (Vec<Json>, i32) {
        let mut output = Vec::new();
        let code = serve(&mut input.as_bytes(), &mut output);
        let text = String::from_utf8(output).unwrap();
        let mut messages = Vec::new();
        let mut rest = text.as_str();
        while let Some((head, tail)) = rest.split_once("\r\n\r\n") {
            let n: usize = head.trim_start_matches("Content-Length: ").parse().unwrap();
            messages.push(Json::parse(&tail[..n]).unwrap());
            rest = &tail[n..];
        }
        (messages, code)
    }

    fn open(uri: &str, text: &str) -> String {
        notify(
            "textDocument/didOpen",
            Json::obj([(
                "textDocument",
                Json::obj([("uri", Json::str(uri)), ("text", Json::str(text))]),
            )]),
        )
    }

    #[test]
    fn definition_hover_and_diagnostics() {
        let uri = "file:///tmp/a.dol";
        // 日本語のコメントの後ろでも、位置がずれない
        let src = "// こんにちは\nadd[@a] (\n    # @a + 1\n)\nlog[add[1]]\n";
        let mut input = String::new();
        input += &call(1, "initialize", Json::obj([]));
        input += &open(uri, src);
        input += &call(2, "textDocument/definition", at(uri, 4, 5));
        input += &call(3, "textDocument/hover", at(uri, 2, 6));
        input += &call(4, "textDocument/documentSymbol", at(uri, 0, 0));
        input += &call(5, "shutdown", Json::Null);
        input += &notify("exit", Json::Null);
        let (out, code) = run(input);
        assert_eq!(code, 0);
        // initialize, diagnostics, definition, hover, symbols, shutdown
        assert_eq!(out.len(), 6);
        assert_eq!(out[1].path("params.diagnostics"), Some(&Json::Arr(vec![])));
        let def = out[2].path("result.range.start").unwrap();
        assert_eq!(def.get("line").unwrap().as_usize(), Some(1));
        assert_eq!(def.get("character").unwrap().as_usize(), Some(0));
        let hover = out[3]
            .path("result.contents.value")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(
            hover.contains("@a := ") || hover.contains("add[@a]"),
            "{hover}"
        );
        let first = out[4].path("result").unwrap();
        let Json::Arr(items) = first else { panic!() };
        assert_eq!(items[0].get("name").unwrap().as_str(), Some("add"));
    }

    fn labels(items: &Json) -> Vec<String> {
        let Json::Arr(items) = items else { panic!() };
        items
            .iter()
            .map(|i| i.get("label").unwrap().as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn completion_in_each_context() {
        let uri = "file:///tmp/d.dol";
        // 5 行目まで解析できる内容を開いてから、書きかけ（構文エラー）にする
        let good = "add[@a] (
    # @a
)
@total := 1
log[@total]
";
        let broken = "add[@a] (
    # @a
)
@total := 1
log[@t
@j := import[\"std:js
@j.pa
log[ad";
        let mut input = open(uri, good);
        input += &notify(
            "textDocument/didChange",
            Json::obj([
                ("textDocument", Json::obj([("uri", Json::str(uri))])),
                (
                    "contentChanges",
                    Json::Arr(vec![Json::obj([("text", Json::str(broken))])]),
                ),
            ]),
        );
        input += &call(1, "textDocument/completion", at(uri, 4, 6));
        input += &call(2, "textDocument/completion", at(uri, 5, 20));
        input += &call(3, "textDocument/completion", at(uri, 7, 6));
        input += &call(4, "textDocument/completion", at(uri, 1, 8));
        let (out, _) = run(input);
        // 0: 診断, 1: 診断, 2〜: 応答
        assert!(labels(out[2].path("result").unwrap()).contains(&"total".to_string()));
        assert!(labels(out[3].path("result").unwrap()).contains(&"std:json".to_string()));
        assert!(!labels(out[4].path("result").unwrap()).is_empty());
        // 関数の中では、トップレベルの変数と引数が見える
        let inside = labels(out[5].path("result").unwrap());
        assert!(
            inside.contains(&"a".to_string()) && inside.contains(&"total".to_string()),
            "{inside:?}"
        );
    }

    #[test]
    fn references_and_rename() {
        let uri = "file:///tmp/e.dol";
        let src = "@n := 1
log[@n + 1]
log[\"v=@n\"]
f[] ( # 1 )
log[f[]]
";
        let mut input = open(uri, src);
        input += &call(1, "textDocument/references", at(uri, 1, 5));
        let mut rename = at(uri, 1, 5);
        if let Json::Obj(f) = &mut rename {
            f.push(("newName".into(), Json::str("count")));
        }
        input += &call(2, "textDocument/rename", rename);
        let mut bad = at(uri, 1, 5);
        if let Json::Obj(f) = &mut bad {
            f.push(("newName".into(), Json::str("1x")));
        }
        input += &call(3, "textDocument/rename", bad);
        input += &call(4, "textDocument/prepareRename", at(uri, 4, 4));
        input += &call(5, "textDocument/prepareRename", at(uri, 4, 0));
        let (out, _) = run(input);
        let Some(Json::Arr(refs)) = out[1].path("result") else {
            panic!()
        };
        assert_eq!(refs.len(), 3);
        let Some(Json::Arr(edits)) = out[2].path("result.changes").and_then(|c| c.get(uri)) else {
            panic!("{:?}", out[2])
        };
        assert_eq!(edits.len(), 3);
        assert_eq!(edits[0].get("newText").unwrap().as_str(), Some("@count"));
        assert!(out[3].path("error.message").is_some());
        assert_eq!(
            out[4].path("result.placeholder").unwrap().as_str(),
            Some("f")
        );
        // `log` は組み込み関数なのでリネームできない
        assert!(out[5].path("error").is_some());
    }

    #[test]
    fn definition_into_a_package() {
        let root = std::env::temp_dir().join("dolphin-lsp-package-test");
        let pkg = root.join("dolphin_modules").join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("mod.dol"),
            "// あいさつ
hello[@n] (
    # @n
)
",
        )
        .unwrap();
        let main = root.join("main.dol");
        let uri = path_to_uri(&main);
        let src = "@p := import[\"pkg\"]
log[@p.hello[1]]
";
        let mut input = open(&uri, src);
        input += &call(1, "textDocument/definition", at(&uri, 1, 7));
        input += &call(2, "textDocument/hover", at(&uri, 1, 7));
        let (out, _) = run(input);
        let target = out[1].path("result.uri").and_then(Json::as_str).unwrap();
        assert!(target.ends_with("dolphin_modules/pkg/mod.dol"), "{target}");
        assert_eq!(
            out[1].path("result.range.start.line").unwrap().as_usize(),
            Some(1)
        );
        let hover = out[2]
            .path("result.contents.value")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(hover.contains("hello[@n]"), "{hover}");
    }

    #[test]
    fn syntax_error_becomes_diagnostic() {
        let uri = "file:///tmp/b.dol";
        let (out, _) = run(open(uri, "@x := (\n"));
        let Some(Json::Arr(list)) = out[0].path("params.diagnostics") else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].path("range.start.line").unwrap().as_usize(),
            Some(0)
        );
    }

    #[test]
    fn unknown_request_is_an_error_and_exit_without_shutdown_is_1() {
        let (out, code) =
            run(call(1, "textDocument/codeAction", Json::Null) + &notify("exit", Json::Null));
        assert_eq!(out[0].path("error.code"), Some(&Json::Num(-32601.0)));
        assert_eq!(code, 1);
    }

    #[test]
    fn formatting_returns_whole_document_edit() {
        let uri = "file:///tmp/c.dol";
        let mut input = open(uri, "@x   :=   1\n");
        input += &call(
            1,
            "textDocument/formatting",
            Json::obj([("textDocument", Json::obj([("uri", Json::str(uri))]))]),
        );
        let (out, _) = run(input);
        let Some(Json::Arr(edits)) = out[1].path("result") else {
            panic!()
        };
        assert_eq!(edits[0].get("newText").unwrap().as_str(), Some("@x := 1\n"));
    }

    #[test]
    fn uri_round_trip() {
        let path = uri_to_path("file:///c%3A/Users/a%20b/x.dol").unwrap();
        assert_eq!(path, PathBuf::from("c:/Users/a b/x.dol"));
        assert_eq!(
            path_to_uri(Path::new("c:/Users/a b/x.dol")),
            "file:///c:/Users/a%20b/x.dol"
        );
    }
}
