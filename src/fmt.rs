//! `dolphin fmt`（design.md 5.18）
//!
//! ソースを字句に分け、空白と字下げだけを決め直す。字句の並びは変えない
//! （整形の前後で字句が同じことを確かめて、違えば結果を返さない）

use crate::diag::Diagnostic;
use crate::syntax::lexer::{self, TokenKind};
use crate::syntax::parser;

const INDENT: &str = "    ";

#[derive(Debug)]
pub enum FmtError {
    /// 構文エラーのあるソースは整形しない
    Syntax(Diagnostic),
    /// 整形で字句が変わってしまった（処理系の誤り）
    Internal(String),
}

/// 整形したソースを返す
pub fn format_source(src: &str) -> Result<String, FmtError> {
    let tokens = lexer::tokenize(src).map_err(FmtError::Syntax)?;
    parser::parse(&tokens).map_err(FmtError::Syntax)?;

    let crlf = src.contains("\r\n");
    let out = format_tokens(&scan(src));
    let out = if crlf { out.replace('\n', "\r\n") } else { out };

    // 字句の並びが変わっていないことを確かめる
    let after = lexer::tokenize(&out)
        .map_err(|e| FmtError::Internal(format!("整形した結果が読めません: {}", e.message)))?;
    if significant(&tokens) != significant(&after) {
        return Err(FmtError::Internal(format!(
            "整形で字句の並びが変わるため中止しました
{:?}
{:?}",
            significant(&tokens),
            significant(&after)
        )));
    }
    Ok(out)
}

/// 比べる字句の並び。連続する改行は 1 つにまとめ、先頭と末尾の改行は無視する
fn significant(tokens: &[lexer::Token]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in tokens {
        let text = format!("{:?}", token.kind);
        if matches!(token.kind, TokenKind::Newline)
            && (out.is_empty() || out.last().is_some_and(|l| *l == text))
        {
            continue;
        }
        out.push(text);
    }
    // ファイルの最後の改行の有無は問わない
    if out.len() >= 2 && out[out.len() - 2] == "Newline" {
        out.remove(out.len() - 2);
    }
    out
}

// ---- 字句 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// 変数・識別子・数値・`true` `false` `nil`
    Word,
    /// `if` `else` `while` `for` `in` `break` `continue`
    Keyword,
    Str,
    Open,
    Close,
    Comma,
    Colon,
    Dot,
    Question,
    Hash,
    Bang,
    /// 二項演算子、`=`、`:=`
    Binary,
    /// 前に値がない `-`
    Neg,
    Comment,
    Newline,
}

#[derive(Debug, Clone)]
struct Tok {
    class: Class,
    text: String,
    /// ブロックの `(` と、それに対応する `)`
    block: bool,
}

/// 検査済み（字句解析が通る）のソースを、空白を捨てて字句に分ける
fn scan(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0;
    let push = |toks: &mut Vec<Tok>, class, text: String| {
        toks.push(Tok {
            class,
            text,
            block: false,
        });
    };
    // 開いている括弧の、字句の番号（ブロックの `(` かどうかを持つ）
    let mut opens: Vec<usize> = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '\n' => {
                push(&mut toks, Class::Newline, "\n".into());
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                push(&mut toks, Class::Comment, text.trim_end().to_string());
            }
            '"' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += if chars[i] == '\\' { 2 } else { 1 };
                }
                i = (i + 1).min(chars.len());
                push(&mut toks, Class::Str, chars[start..i].iter().collect());
            }
            '@' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                push(&mut toks, Class::Word, chars[start..i].iter().collect());
            }
            c if c.is_ascii_digit() => {
                // 検査済みなので、数字・英字・`_` と、小数点・指数の符号をまとめて取る
                let start = i;
                while i < chars.len() {
                    let ch = chars[i];
                    let sign = matches!(ch, '+' | '-')
                        && matches!(chars[i - 1], 'e' | 'E')
                        && !is_hex_literal(&chars[start..i]);
                    let dot = ch == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit);
                    if ch.is_ascii_alphanumeric() || ch == '_' || sign || dot {
                        i += 1;
                    } else {
                        break;
                    }
                }
                push(&mut toks, Class::Word, chars[start..i].iter().collect());
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let class = match word.as_str() {
                    "if" | "else" | "while" | "for" | "in" | "break" | "continue" => Class::Keyword,
                    _ => Class::Word,
                };
                push(&mut toks, class, word);
            }
            _ => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let (class, len) = match two.as_str() {
                    ":=" | "==" | "!=" | "<=" | ">=" | "&&" | "||" => (Class::Binary, 2),
                    _ => (
                        match c {
                            '(' | '[' | '{' => Class::Open,
                            ')' | ']' | '}' => Class::Close,
                            ',' => Class::Comma,
                            ':' => Class::Colon,
                            '.' => Class::Dot,
                            '?' => Class::Question,
                            '#' => Class::Hash,
                            '!' => Class::Bang,
                            // 単項かどうかは、前の字句を見て決める
                            '-' => Class::Binary,
                            _ => Class::Binary,
                        },
                        1,
                    ),
                };
                let text: String = chars[i..i + len].iter().collect();
                // `-` は、前に値がなければ単項（後ろに空白を入れない）
                let class = if text == "-" && !ends_value(&toks) {
                    Class::Neg
                } else {
                    class
                };
                // ブロックの `(` は、値の終わりか `else` の直後（`if @a (` の `@a` が値の終わり）。
                // `if (@a + @b) > 0 (` の最初の `(` は、式をまとめる括弧
                let is_block_open = text == "("
                    && (ends_value(&toks) || toks.last().is_some_and(|t| t.text == "else"));
                push(&mut toks, class, text);
                match class {
                    Class::Open => {
                        toks.last_mut().expect("push したばかり").block = is_block_open;
                        opens.push(toks.len() - 1);
                    }
                    Class::Close => {
                        if let Some(open) = opens.pop() {
                            let block = toks[open].block;
                            toks.last_mut().expect("push したばかり").block = block;
                        }
                    }
                    _ => {}
                }
                i += len;
            }
        }
    }
    toks
}

/// `0x…` の 16 進数リテラルか（`e` が桁で、指数ではない）
fn is_hex_literal(chars: &[char]) -> bool {
    chars.starts_with(&['0', 'x'])
}

/// 直前の字句が値の終わりか
fn ends_value(toks: &[Tok]) -> bool {
    matches!(
        toks.last().map(|t| t.class),
        Some(Class::Word | Class::Str | Class::Close | Class::Question)
    )
}

// ---- 空白 ----

fn is_value_end(class: Class) -> bool {
    matches!(
        class,
        Class::Word | Class::Str | Class::Close | Class::Question
    )
}

/// 同じ行で `prev` の次に `next` を置くとき、間に空白を入れるか
fn gap(prev: &Tok, next: &Tok) -> bool {
    use Class::*;
    // 1 行に書いたブロック `( # 1 )` は、内側に空白を入れる（空のブロックは `()`）
    if prev.class == Open && prev.block {
        return next.class != Close;
    }
    if next.class == Close && next.block {
        return true;
    }
    if matches!(next.class, Close | Comma | Colon | Dot | Question) {
        return false;
    }
    if matches!(prev.class, Open | Dot | Bang | Neg) {
        return false;
    }
    if matches!(prev.class, Comma | Colon | Binary | Hash | Keyword) {
        return true;
    }
    if matches!(next.class, Binary) {
        return true;
    }
    if next.class == Open {
        // ブロックの `(` は離す。呼び出し `f[` と添字 `@a{` は詰める
        return next.text == "(" && is_value_end(prev.class);
    }
    is_value_end(prev.class) && matches!(next.class, Word | Str | Keyword)
}

// ---- 行の組み立て ----

fn format_tokens(toks: &[Tok]) -> String {
    // 行に分ける。連続する改行は、空行があったことだけ覚える
    let mut lines: Vec<Vec<&Tok>> = Vec::new();
    let mut blank_before: Vec<bool> = Vec::new();
    let mut current: Vec<&Tok> = Vec::new();
    let mut newlines = 0;
    let mut pending_blank = false;
    for tok in toks {
        if tok.class == Class::Newline {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                blank_before.push(pending_blank);
            }
            newlines += 1;
            continue;
        }
        if current.is_empty() {
            pending_blank = newlines >= 2;
        }
        newlines = 0;
        current.push(tok);
    }
    if !current.is_empty() {
        lines.push(current);
        blank_before.push(pending_blank);
    }

    let mut out = String::new();
    // 開いている括弧の、開いた行の番号
    let mut stack: Vec<usize> = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        // 空行は 1 つまで。先頭、開き括弧の直後、閉じ括弧の直前では消す
        let starts_with_close = line[0].class == Class::Close;
        let prev_ends_open = n > 0 && lines[n - 1].last().is_some_and(|t| t.class == Class::Open);
        if blank_before[n] && n > 0 && !starts_with_close && !prev_ends_open {
            out.push('\n');
        }

        // 行頭の閉じ括弧の分を先に戻して、字下げを決める
        let leading_closers = line.iter().take_while(|t| t.class == Class::Close).count();
        let open_after = &stack[..stack.len().saturating_sub(leading_closers)];
        let level = open_after
            .iter()
            .enumerate()
            .filter(|(i, l)| *i == 0 || open_after[i - 1] != **l)
            .count();
        for _ in 0..level {
            out.push_str(INDENT);
        }

        let mut prev: Option<&Tok> = None;
        for tok in line {
            if let Some(p) = prev
                && (tok.class == Class::Comment || gap(p, tok))
            {
                out.push(' ');
            }
            out.push_str(&tok.text);
            match tok.class {
                Class::Open => stack.push(n),
                Class::Close => {
                    stack.pop();
                }
                _ => {}
            }
            prev = Some(tok);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        match format_source(src) {
            Ok(out) => out,
            Err(e) => panic!("{e:?}"),
        }
    }

    #[test]
    fn spaces_around_operators_and_punctuation() {
        assert_eq!(
            fmt("@x:=1+2*@y\n@a  =  {1,2 ,3}\n@m={ \"a\":1,\"b\" :2 }\n@e={:}\n"),
            "@x := 1 + 2 * @y\n@a = {1, 2, 3}\n@m = {\"a\": 1, \"b\": 2}\n@e = {:}\n"
        );
        assert_eq!(
            fmt("log[ @a{0}{1} , f[ 1 ]  ,@u.name[ 2 ] ]\n"),
            "log[@a{0}{1}, f[1], @u.name[2]]\n"
        );
    }

    #[test]
    fn unary_operators_and_try() {
        assert_eq!(
            fmt("@x=-1\n@y=@a - -@b\n@z= !@c\n# -@x\n@t=file_read[@p] ?\n"),
            "@x = -1\n@y = @a - -@b\n@z = !@c\n# -@x\n@t = file_read[@p]?\n"
        );
        assert_eq!(fmt("@x=(1+2)*-3\n"), "@x = (1 + 2) * -3\n");
    }

    #[test]
    fn blocks_and_indentation() {
        let src = "add[@a,@b](\n# @a+@b\n)\nif @x>1(\nlog[1]\n)else(\n    log[2]\n)\nwhile true (\n        break\n   )\n";
        assert_eq!(
            fmt(src),
            "add[@a, @b] (\n    # @a + @b\n)\nif @x > 1 (\n    log[1]\n) else (\n    log[2]\n)\nwhile true (\n    break\n)\n"
        );
        // `)` と `else` を別の行に書いたら、そのまま
        assert_eq!(
            fmt("if true (\nlog[1]\n)\nelse (\nlog[2]\n)\n"),
            "if true (\n    log[1]\n)\nelse (\n    log[2]\n)\n"
        );
    }

    #[test]
    fn multiline_literals_indent_once_per_line() {
        assert_eq!(
            fmt("@m := {\n\"a\": 1,\n\"b\": {\n\"c\": 2\n}\n}\nlog[@a, {\n1,\n2\n}]\n"),
            "@m := {\n    \"a\": 1,\n    \"b\": {\n        \"c\": 2\n    }\n}\nlog[@a, {\n    1,\n    2\n}]\n"
        );
    }

    #[test]
    fn inline_blocks_and_lambdas() {
        assert_eq!(
            fmt("@f:=[@x]( # @x+1 )\n@g := arr_map[@a,[@v](\n# @v*2\n)]\n"),
            "@f := [@x] ( # @x + 1 )\n@g := arr_map[@a, [@v] (\n    # @v * 2\n)]\n"
        );
    }

    #[test]
    fn comments_blank_lines_and_eof() {
        assert_eq!(
            fmt("\n\n// 先頭\n\n\n@x=1   //  行末\nf[](\n\n    // 中\n\n\n    log[1]\n\n)\n\n\n"),
            "// 先頭\n\n@x = 1 //  行末\nf[] (\n    // 中\n\n    log[1]\n)\n"
        );
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("@x=1"), "@x = 1\n");
    }

    #[test]
    fn strings_and_numbers_are_kept() {
        assert_eq!(
            fmt("log[\"a  b // c \\\" @x\",1.50 ,\"\"]\n"),
            "log[\"a  b // c \\\" @x\", 1.50, \"\"]\n"
        );
    }

    #[test]
    fn number_literals_are_kept() {
        assert_eq!(
            fmt("@x=0xFF+1_000+1e-3-2E+3*0xe+1
"),
            "@x = 0xFF + 1_000 + 1e-3 - 2E+3 * 0xe + 1
"
        );
    }

    #[test]
    fn line_endings_are_kept() {
        assert_eq!(fmt("@x=1\r\n@y=2\r\n"), "@x = 1\r\n@y = 2\r\n");
    }

    #[test]
    fn keywords_and_for() {
        assert_eq!(
            fmt(
                "for @x in{1,2}(\ncontinue\n)\nfor @y in range[0,3] (\nlog[@y]\n)\nif !@a&&@b||@c(\nbreak\n)\n"
            ),
            "for @x in {1, 2} (\n    continue\n)\nfor @y in range[0, 3] (\n    log[@y]\n)\nif !@a && @b || @c (\n    break\n)\n"
        );
    }

    #[test]
    fn idempotent() {
        let src = "f[@a](\nif @a>1(\nlog[{1,\n2}]\n)else if @a (\n# -1\n)\n)\n@m={\"k\":[@x](\n# @x\n)}\n";
        let once = fmt(src);
        assert_eq!(fmt(&once), once);
    }

    #[test]
    fn syntax_errors_are_not_formatted() {
        assert!(matches!(
            format_source("@x := (\n"),
            Err(FmtError::Syntax(_))
        ));
    }
}
