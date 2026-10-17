//! 補完（design.md 5.34）。カーソルの前の文字から、何を補うかを決める

use std::collections::HashSet;
use std::path::Path;

use super::analysis::{Analysis, DefKind, analyze};
use super::json::Json;
use crate::Catalog;

const KEYWORDS: &[&str] = &[
    "if", "else", "while", "for", "in", "break", "continue", "true", "false", "nil",
];

// CompletionItemKind
const FUNCTION: usize = 3;
const VARIABLE: usize = 6;
const MODULE: usize = 9;
const KEYWORD: usize = 14;

/// カーソルの前にあるもの
#[derive(Debug, PartialEq)]
enum Context {
    /// `import["std:` の途中。`start` は `std:` の先頭の文字位置（0 始まり）
    Module { start: usize },
    /// `@m.partial`
    Member { var: String },
    /// `@partial`
    Variable,
    /// 名前の途中
    Name,
    /// コメントの中、文字列の中など、補わない所
    None,
}

fn context(before: &[char]) -> Context {
    // コメントと文字列の中かどうかを、行の先頭から数える
    let mut in_string = false;
    let mut string_start = 0;
    let mut i = 0;
    while i < before.len() {
        match before[i] {
            '/' if !in_string && before.get(i + 1) == Some(&'/') => return Context::None,
            '"' => {
                in_string = !in_string;
                string_start = i + 1;
            }
            '\\' if in_string => i += 1,
            _ => {}
        }
        i += 1;
    }
    if in_string {
        let text: String = before[string_start..].iter().collect();
        if let Some(rest) = text.strip_prefix("std:")
            && rest.chars().all(is_ident)
        {
            return Context::Module {
                start: string_start,
            };
        }
        // 文字列の中で補うのは、`@name` だけ
        return match variable_prefix(before) {
            Some(_) => Context::Variable,
            None => Context::None,
        };
    }
    // `@m.partial`
    let mut j = before.len();
    while j > 0 && is_ident(before[j - 1]) {
        j -= 1;
    }
    if j >= 2 && before[j - 1] == '.' {
        let end = j - 1;
        let mut k = end;
        while k > 0 && is_ident(before[k - 1]) {
            k -= 1;
        }
        if k > 0 && before[k - 1] == '@' && k < end {
            return Context::Member {
                var: before[k..end].iter().collect(),
            };
        }
    }
    if variable_prefix(before).is_some() {
        Context::Variable
    } else {
        Context::Name
    }
}

/// 末尾が `@ident`（ident は空でもよい）なら、その `@` の位置
fn variable_prefix(before: &[char]) -> Option<usize> {
    let mut j = before.len();
    while j > 0 && is_ident(before[j - 1]) {
        j -= 1;
    }
    (j > 0 && before[j - 1] == '@').then(|| j - 1)
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn item(label: &str, kind: usize, detail: Option<&str>) -> Vec<(String, Json)> {
    let mut fields = vec![
        ("label".to_string(), Json::str(label)),
        ("kind".to_string(), Json::num(kind)),
    ];
    if let Some(detail) = detail {
        fields.push(("detail".to_string(), Json::str(detail)));
    }
    fields
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// `line`（1 始まり）にカーソルがあるときの補完候補。`text_before` はその行のカーソルより前、
/// `line_text` は行全体。`dir` は、この文書の `import` の基準
pub fn complete(
    analysis: &Analysis,
    catalog: &Catalog,
    dir: Option<&Path>,
    text_before: &str,
    line: usize,
    line_text: &str,
) -> Json {
    let before: Vec<char> = text_before.chars().collect();
    let mut items: Vec<Vec<(String, Json)>> = Vec::new();
    match context(&before) {
        Context::None => {}
        Context::Module { start } => {
            // `std:` ごと置き換える
            let from = line_text
                .chars()
                .take(start)
                .map(char::len_utf16)
                .sum::<usize>();
            let to = text_before.encode_utf16().count();
            let at = |character: usize| {
                Json::obj([
                    ("line", Json::num(line - 1)),
                    ("character", Json::num(character)),
                ])
            };
            for (name, _) in &catalog.modules {
                let mut fields = item(name, MODULE, None);
                fields.push((
                    "textEdit".to_string(),
                    Json::obj([
                        ("range", Json::obj([("start", at(from)), ("end", at(to))])),
                        ("newText", Json::str(*name)),
                    ]),
                ));
                items.push(fields);
            }
        }
        Context::Member { var } => {
            let import = analysis
                .defs
                .iter()
                .rev()
                .find(|d| d.kind == DefKind::Var && d.name == var)
                .and_then(|d| d.import.clone());
            match import {
                Some(path) if path.starts_with("std:") => {
                    if let Some((_, members)) = catalog.modules.iter().find(|(n, _)| *n == path) {
                        items.extend(members.iter().map(|m| item(m, FUNCTION, Some(&path))));
                    }
                }
                Some(path) => {
                    let exports = dir
                        .and_then(|d| crate::runtime::module::resolve_import(d, &path).ok())
                        .and_then(|file| std::fs::read_to_string(file).ok())
                        .and_then(|text| analyze(&text).ok())
                        .map(|a| a.exports);
                    if let Some(exports) = exports {
                        let mut defs: Vec<_> = exports.values().collect();
                        defs.sort_by_key(|d| d.span.line);
                        items.extend(defs.into_iter().map(|d| {
                            let kind = if d.kind == DefKind::Func {
                                FUNCTION
                            } else {
                                VARIABLE
                            };
                            item(&d.name, kind, Some(first_line(&d.hover)))
                        }));
                    }
                }
                None => {}
            }
        }
        Context::Variable => {
            let funcs = || analysis.symbols.iter().filter(|s| s.kind == DefKind::Func);
            let inside_any_func = |l: usize| funcs().any(|s| s.first_line <= l && l <= s.last_line);
            let current = funcs().find(|s| s.first_line <= line && line <= s.last_line);
            let mut seen = HashSet::new();
            for d in analysis.defs.iter().filter(|d| d.kind == DefKind::Var) {
                let visible = match current {
                    // 関数の中: その関数の中でカーソルより前の宣言と、トップレベルの変数
                    Some(f) => {
                        (f.first_line <= d.span.line
                            && d.span.line <= f.last_line
                            && d.span.line < line)
                            || !inside_any_func(d.span.line)
                    }
                    // 関数の外: カーソルより前のトップレベルの宣言
                    None => !inside_any_func(d.span.line) && d.span.line < line,
                };
                if visible && seen.insert(d.name.clone()) {
                    items.push(item(&d.name, VARIABLE, Some(first_line(&d.hover))));
                }
            }
        }
        Context::Name => {
            let mut seen = HashSet::new();
            for d in analysis.defs.iter().filter(|d| d.kind == DefKind::Func) {
                if seen.insert(d.name.clone()) {
                    items.push(item(&d.name, FUNCTION, Some(first_line(&d.hover))));
                }
            }
            for name in &catalog.functions {
                if seen.insert((*name).to_string()) {
                    items.push(item(name, FUNCTION, Some("組み込み関数")));
                }
            }
            items.extend(KEYWORDS.iter().map(|k| item(k, KEYWORD, None)));
        }
    }
    Json::Arr(items.into_iter().map(Json::Obj).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(s: &str) -> Context {
        context(&s.chars().collect::<Vec<_>>())
    }

    #[test]
    fn contexts() {
        assert_eq!(ctx("log[@"), Context::Variable);
        assert_eq!(ctx("log[@na"), Context::Variable);
        assert_eq!(ctx("@j := import[\"std:js"), Context::Module { start: 14 });
        assert_eq!(ctx("log[@j.pa"), Context::Member { var: "j".into() });
        assert_eq!(ctx("@j."), Context::Member { var: "j".into() });
        assert_eq!(ctx("arr_pu"), Context::Name);
        assert_eq!(ctx("// コメント @"), Context::None);
        assert_eq!(ctx("log[\"hello wor"), Context::None);
        assert_eq!(ctx("log[\"hello @na"), Context::Variable);
        assert_eq!(ctx("log[\"a // b\"] + ar"), Context::Name);
    }
}
