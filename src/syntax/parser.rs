//! トークン列から AST を作る。design.md 4 章の EBNF を再帰下降で実装する
//!
//! 文法の規則 1 つにつき関数 1 つが対応する（`statement`、`block`、`or_expr` など）

use std::rc::Rc;

use super::ast::{BinaryOp, Block, Expr, ExprKind, FuncDef, Program, Stmt, StmtKind, UnaryOp, Var};
use super::lexer::{StrPart, Token, TokenKind};
use super::span::Span;
use crate::diag::Diagnostic;

type PResult<T> = Result<T, Diagnostic>;

pub fn parse(tokens: &[Token]) -> PResult<Program> {
    let mut parser = Parser { tokens, pos: 0 };
    let stmts = parser.statements_until(&TokenKind::Eof)?;
    Ok(Program { stmts })
}

struct Parser<'t> {
    /// 末尾は必ず Eof
    tokens: &'t [Token],
    pos: usize,
}

impl<'t> Parser<'t> {
    // ---- トークン操作 ----

    fn peek(&self) -> &'t Token {
        &self.tokens[self.pos]
    }

    fn at(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    /// 1 つ読み進めて、読んだトークンを返す。Eof からは進まない
    fn advance(&mut self) -> &'t Token {
        let token = self.peek();
        if token.kind != TokenKind::Eof {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> PResult<Span> {
        if self.at(kind) {
            Ok(self.advance().span)
        } else {
            Err(self.unexpected(&kind.to_string()))
        }
    }

    /// 「{expected}が必要です（{今のトークン}がありました）」
    fn unexpected(&self, expected: &str) -> Diagnostic {
        let token = self.peek();
        let diag = Diagnostic::new(
            token.span,
            format!("{expected}が必要です（{}がありました）", token.kind),
        );
        if token.kind == TokenKind::Eof {
            diag.at_eof()
        } else {
            diag
        }
    }

    fn skip_newlines(&mut self) {
        while self.eat(&TokenKind::Newline) {}
    }

    // ---- 文 ----

    /// `end` の手前まで、改行で区切られた文を読む。`end` 自体は読まない
    fn statements_until(&mut self, end: &TokenKind) -> PResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.at(end) && !self.at(&TokenKind::Eof) {
            stmts.push(self.statement()?);
            if self.at(end) || self.at(&TokenKind::Eof) {
                break;
            }
            // 1 行に文は 1 つ（design.md 4.1）
            if !self.at(&TokenKind::Newline) {
                return Err(self.unexpected("改行"));
            }
            self.skip_newlines();
        }
        Ok(stmts)
    }

    /// block = "(" { NEWLINE } [ statement { NEWLINE { NEWLINE } statement } ] { NEWLINE } ")"
    fn block(&mut self) -> PResult<Block> {
        self.expect(&TokenKind::LParen)?;
        let stmts = self.statements_until(&TokenKind::RParen)?;
        self.expect(&TokenKind::RParen)?;
        Ok(stmts)
    }

    fn statement(&mut self) -> PResult<Stmt> {
        let span = self.peek().span;
        let kind = match self.peek().kind {
            TokenKind::If => return self.if_stmt(),
            TokenKind::While => {
                self.advance();
                let cond = self.expression()?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            // for_stmt = "for" variable "in" expression block
            TokenKind::For => {
                self.advance();
                let TokenKind::Var(name) = &self.peek().kind else {
                    return Err(self.unexpected("変数（`@x`）"));
                };
                let var = Var(name.clone());
                self.advance();
                self.expect(&TokenKind::In)?;
                let iter = self.expression()?;
                let body = self.block()?;
                StmtKind::For { var, iter, body }
            }
            TokenKind::Hash => {
                self.advance();
                let value = match self.peek().kind {
                    TokenKind::Newline | TokenKind::RParen | TokenKind::Eof => None,
                    _ => Some(self.expression()?),
                };
                StmtKind::Return(value)
            }
            TokenKind::Break => {
                self.advance();
                StmtKind::Break
            }
            TokenKind::Continue => {
                self.advance();
                StmtKind::Continue
            }
            _ => return self.simple_statement(),
        };
        Ok(Stmt { kind, span })
    }

    /// 代入・関数定義・式文。どれも式から始まるので、まず式として読み、
    /// 続くトークンで区別する
    ///
    /// - `:=` が続く → 宣言。左辺は変数に限る
    /// - `=` が続く → 代入。左辺は変数か `変数{式}...` に限る
    /// - `(` が続き、左辺が呼び出しの形 → 関数定義（design.md 4.1）
    fn simple_statement(&mut self) -> PResult<Stmt> {
        let span = self.peek().span;
        let expr = self.expression()?;

        let kind = if self.at(&TokenKind::Declare) {
            let ExprKind::Var(target) = expr.kind else {
                return Err(Diagnostic::new(expr.span, "宣言できるのは変数だけです"));
            };
            self.advance();
            let value = self.expression()?;
            StmtKind::Declare { target, value }
        } else if self.at(&TokenKind::Assign) {
            let (target, indices) = assign_target(expr)?;
            self.advance();
            let value = self.expression()?;
            StmtKind::Assign {
                target,
                indices,
                value,
            }
        } else if self.at(&TokenKind::LParen) {
            let ExprKind::Call { name, args } = expr.kind else {
                return Err(self.unexpected("改行"));
            };
            StmtKind::FuncDef(Rc::new(self.func_def(name, args, span)?))
        } else {
            StmtKind::Expr(expr)
        };
        Ok(Stmt { kind, span })
    }

    /// func_def = IDENT "[" [ params ] "]" block
    /// 引数部分は呼び出しとして読み済みなので、変数だけかを確かめる
    fn func_def(&mut self, name: String, args: Vec<Expr>, span: Span) -> PResult<FuncDef> {
        let params = args
            .into_iter()
            .map(|arg| match arg.kind {
                ExprKind::Var(Var(param)) => Ok(param),
                _ => Err(Diagnostic::new(
                    arg.span,
                    "関数定義の引数は変数（`@x`）で書きます",
                )),
            })
            .collect::<PResult<Vec<_>>>()?;
        let body = self.block()?;
        Ok(FuncDef {
            name,
            params,
            body,
            span,
        })
    }

    /// if_stmt = "if" expression block [ { NEWLINE } "else" ( block | if_stmt ) ]
    fn if_stmt(&mut self) -> PResult<Stmt> {
        let span = self.expect(&TokenKind::If)?;
        let cond = self.expression()?;
        let then_block = self.block()?;
        let else_block = if self.else_follows() {
            if self.at(&TokenKind::If) {
                Some(vec![self.if_stmt()?])
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(Stmt {
            kind: StmtKind::If {
                cond,
                then_block,
                else_block,
            },
            span,
        })
    }

    /// 改行を挟んで `else` が続くなら、`else` まで読み進めて true を返す。
    /// 続かなければ位置を戻す（改行は文の区切りとして残す）
    fn else_follows(&mut self) -> bool {
        let saved = self.pos;
        self.skip_newlines();
        if self.eat(&TokenKind::Else) {
            true
        } else {
            self.pos = saved;
            false
        }
    }

    // ---- 式（下ほど優先度が高い） ----

    fn expression(&mut self) -> PResult<Expr> {
        self.or_expr()
    }

    /// or_expr = and_expr { "||" and_expr }
    fn or_expr(&mut self) -> PResult<Expr> {
        self.left_assoc(Self::and_expr, |kind| match kind {
            TokenKind::OrOr => Some(BinaryOp::Or),
            _ => None,
        })
    }

    /// and_expr = cmp_expr { "&&" cmp_expr }
    fn and_expr(&mut self) -> PResult<Expr> {
        self.left_assoc(Self::cmp_expr, |kind| match kind {
            TokenKind::AndAnd => Some(BinaryOp::And),
            _ => None,
        })
    }

    /// cmp_expr = add_expr [ 比較演算子 add_expr ]
    /// 比較は連鎖させない（`1 < @x < 3` はエラー）
    fn cmp_expr(&mut self) -> PResult<Expr> {
        let lhs = self.add_expr()?;
        let Some(op) = cmp_op(&self.peek().kind) else {
            return Ok(lhs);
        };
        let span = self.advance().span;
        let rhs = self.add_expr()?;
        if cmp_op(&self.peek().kind).is_some() {
            return Err(Diagnostic::new(
                self.peek().span,
                "比較は続けて書けません（`&&` でつないでください）",
            ));
        }
        Ok(binary(op, lhs, rhs, span))
    }

    /// add_expr = mul_expr { ( "+" | "-" ) mul_expr }
    fn add_expr(&mut self) -> PResult<Expr> {
        self.left_assoc(Self::mul_expr, |kind| match kind {
            TokenKind::Plus => Some(BinaryOp::Add),
            TokenKind::Minus => Some(BinaryOp::Sub),
            _ => None,
        })
    }

    /// mul_expr = unary { ( "*" | "/" | "%" ) unary }
    fn mul_expr(&mut self) -> PResult<Expr> {
        self.left_assoc(Self::unary, |kind| match kind {
            TokenKind::Star => Some(BinaryOp::Mul),
            TokenKind::Slash => Some(BinaryOp::Div),
            TokenKind::Percent => Some(BinaryOp::Rem),
            _ => None,
        })
    }

    /// 左結合の二項演算 `operand { op operand }` の共通処理
    fn left_assoc(
        &mut self,
        operand: fn(&mut Self) -> PResult<Expr>,
        op_of: fn(&TokenKind) -> Option<BinaryOp>,
    ) -> PResult<Expr> {
        let mut lhs = operand(self)?;
        while let Some(op) = op_of(&self.peek().kind) {
            let span = self.advance().span;
            let rhs = operand(self)?;
            lhs = binary(op, lhs, rhs, span);
        }
        Ok(lhs)
    }

    /// unary = ( "-" | "!" ) unary | postfix
    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek().kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Bang => UnaryOp::Not,
            _ => return self.postfix(),
        };
        let span = self.advance().span;
        let operand = self.unary()?;
        Ok(Expr {
            kind: ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            span,
        })
    }

    /// postfix = primary { "{" expression "}" | "[" [ args ] "]" | "." member | "?" }
    /// member  = IDENT [ "[" [ args ] "]" ]
    fn postfix(&mut self) -> PResult<Expr> {
        let mut expr = self.primary()?;
        loop {
            let span = expr.span;
            if self.eat(&TokenKind::LBrace) {
                let index = self.expression()?;
                self.expect(&TokenKind::RBrace)?;
                expr = Expr {
                    kind: ExprKind::Index {
                        target: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.eat(&TokenKind::LBracket) {
                let args = self.args(&TokenKind::RBracket)?;
                expr = Expr {
                    kind: ExprKind::CallValue {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.eat(&TokenKind::Question) {
                expr = Expr {
                    kind: ExprKind::Try(Box::new(expr)),
                    span,
                };
            } else if self.eat(&TokenKind::Dot) {
                let TokenKind::Ident(name) = &self.peek().kind else {
                    return Err(self.unexpected("名前"));
                };
                let name = name.clone();
                self.advance();
                let target = Box::new(expr);
                let kind = if self.eat(&TokenKind::LBracket) {
                    let args = self.args(&TokenKind::RBracket)?;
                    ExprKind::MemberCall { target, name, args }
                } else {
                    ExprKind::Member { target, name }
                };
                expr = Expr { kind, span };
            } else {
                return Ok(expr);
            }
        }
    }

    /// `( 式 )`。式の前後の改行は読み飛ばす
    fn paren_expr(&mut self) -> PResult<Expr> {
        self.expect(&TokenKind::LParen)?;
        self.skip_newlines();
        let expr = self.expression()?;
        self.skip_newlines();
        self.expect(&TokenKind::RParen)?;
        Ok(expr)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let token = self.peek();
        let kind = match &token.kind {
            TokenKind::Number(n) => ExprKind::Number(*n),
            TokenKind::Str(parts) => ExprKind::Str(parts.clone()),
            TokenKind::True => ExprKind::Bool(true),
            TokenKind::False => ExprKind::Bool(false),
            TokenKind::Nil => ExprKind::Nil,
            TokenKind::Var(name) => ExprKind::Var(Var(name.clone())),
            // call = IDENT "[" [ args ] "]"。`[` が続かなければ、関数名だけ（関数値）
            TokenKind::Ident(name) => {
                self.advance();
                let kind = if self.eat(&TokenKind::LBracket) {
                    ExprKind::Call {
                        name: name.clone(),
                        args: self.args(&TokenKind::RBracket)?,
                    }
                } else {
                    ExprKind::FuncRef(name.clone())
                };
                return Ok(Expr {
                    kind,
                    span: token.span,
                });
            }
            // cond_expr = "if" expression "(" expression ")" "else" ( "(" expression ")" | cond_expr )
            TokenKind::If => {
                self.advance();
                let cond = self.expression()?;
                let then_expr = self.paren_expr()?;
                if !self.at(&TokenKind::Else) {
                    return Err(self.unexpected("式の `if` の `else`"));
                }
                self.advance();
                let else_expr = if self.at(&TokenKind::If) {
                    self.primary()?
                } else {
                    self.paren_expr()?
                };
                return Ok(Expr {
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                    },
                    span: token.span,
                });
            }
            // lambda = "[" [ params ] "]" block
            TokenKind::LBracket => {
                self.advance();
                let params = self.params()?;
                let body = self.block()?;
                let def = FuncDef {
                    name: String::new(),
                    params,
                    body,
                    span: token.span,
                };
                return Ok(Expr {
                    kind: ExprKind::Lambda(Rc::new(def)),
                    span: token.span,
                });
            }
            TokenKind::LBrace => {
                self.advance();
                return self.brace_literal(token.span);
            }
            // "(" expression ")"
            TokenKind::LParen => {
                self.advance();
                let expr = self.expression()?;
                self.expect(&TokenKind::RParen)?;
                return Ok(expr);
            }
            _ => return Err(self.unexpected("式")),
        };
        self.advance();
        Ok(Expr {
            kind,
            span: token.span,
        })
    }

    /// params = VAR { "," VAR }。開き括弧の後に呼ぶ。`]` まで読み進める
    fn params(&mut self) -> PResult<Vec<String>> {
        let mut params = Vec::new();
        if self.eat(&TokenKind::RBracket) {
            return Ok(params);
        }
        loop {
            let TokenKind::Var(name) = &self.peek().kind else {
                return Err(self.unexpected("変数（`@x`）"));
            };
            params.push(name.clone());
            self.advance();
            if self.eat(&TokenKind::RBracket) {
                return Ok(params);
            }
            if !self.eat(&TokenKind::Comma) {
                return Err(self.unexpected("`,` か `]`"));
            }
        }
    }

    /// array_lit = "{" [ args ] "}"
    /// map_lit   = "{" ":" "}" | "{" entry { "," entry } "}"
    ///
    /// `{` の後に呼ぶ。最初の要素の後に `:` があればマップ、なければ配列（design.md 4.1）
    fn brace_literal(&mut self, span: Span) -> PResult<Expr> {
        let kind = if self.eat(&TokenKind::Colon) {
            self.expect(&TokenKind::RBrace)?;
            ExprKind::Map(Vec::new())
        } else if self.eat(&TokenKind::RBrace) {
            ExprKind::Array(Vec::new())
        } else {
            let first = self.expression()?;
            if self.eat(&TokenKind::Colon) {
                let mut entries = vec![(first, self.expression()?)];
                while !self.eat(&TokenKind::RBrace) {
                    if !self.eat(&TokenKind::Comma) {
                        return Err(self.unexpected("`,` か `}`"));
                    }
                    let key = self.expression()?;
                    self.expect(&TokenKind::Colon)?;
                    entries.push((key, self.expression()?));
                }
                ExprKind::Map(entries)
            } else {
                let mut items = vec![first];
                while !self.eat(&TokenKind::RBrace) {
                    if !self.eat(&TokenKind::Comma) {
                        return Err(self.unexpected("`,` か `}`"));
                    }
                    items.push(self.expression()?);
                }
                ExprKind::Array(items)
            }
        };
        Ok(Expr { kind, span })
    }

    /// args = expression { "," expression }
    /// 開き括弧の後に呼ぶ。閉じ括弧 `end` まで読み進める
    fn args(&mut self, end: &TokenKind) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        if self.eat(end) {
            return Ok(args);
        }
        loop {
            args.push(self.expression()?);
            if self.eat(end) {
                return Ok(args);
            }
            if !self.eat(&TokenKind::Comma) {
                return Err(self.unexpected(&format!("`,` か {end}")));
            }
        }
    }
}

fn cmp_op(kind: &TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::Eq => Some(BinaryOp::Eq),
        TokenKind::NotEq => Some(BinaryOp::NotEq),
        TokenKind::Lt => Some(BinaryOp::Lt),
        TokenKind::LtEq => Some(BinaryOp::LtEq),
        TokenKind::Gt => Some(BinaryOp::Gt),
        TokenKind::GtEq => Some(BinaryOp::GtEq),
        _ => None,
    }
}

/// 二項演算の Expr を作る。位置は演算子の位置にする（型エラーで演算子を指すため）
fn binary(op: BinaryOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        span,
    }
}

/// 代入の左辺を (変数, 添字を外側から順に並べたもの) に分解する
fn assign_target(expr: Expr) -> PResult<(Var, Vec<Expr>)> {
    let invalid = || Diagnostic::new(expr.span, "代入できるのは変数か配列の要素だけです");
    let mut indices = Vec::new();
    let mut current = expr.kind;
    loop {
        match current {
            // `@m.key = v` は `@m{"key"} = v`。モジュールかどうかは実行時に分かる
            ExprKind::Member { target, name } => {
                indices.push(Expr {
                    kind: ExprKind::Str(vec![StrPart::Text(name)]),
                    span: target.span,
                });
                current = target.kind;
            }
            ExprKind::Var(var) => {
                indices.reverse();
                return Ok((var, indices));
            }
            ExprKind::Index { target, index } => {
                indices.push(*index);
                current = target.kind;
            }
            _ => return Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::lexer::tokenize;

    /// AST を S 式の文字列にする
    fn ast(src: &str) -> String {
        parse(&tokenize(src).unwrap()).unwrap().to_string()
    }

    fn error(src: &str) -> Diagnostic {
        parse(&tokenize(src).unwrap()).unwrap_err()
    }

    #[test]
    fn precedence() {
        assert_eq!(
            ast("@x = 1 + 2 * 3 - -@y"),
            "(= @x (- (+ 1 (* 2 3)) (- @y)))\n"
        );
        assert_eq!(
            ast("@ok = !@a || @b && @c >= 1"),
            "(= @ok (|| (! @a) (&& @b (>= @c 1))))\n"
        );
    }

    #[test]
    fn grouping_and_block() {
        // 先頭の ( はグループ化、最後の ( はブロック
        assert_eq!(
            ast("if (@a + @b) * 2 > 0 (\n    log[\"big\"]\n)"),
            "(if (> (* (+ @a @b) 2) 0) ((log \"big\")))\n"
        );
    }

    #[test]
    fn func_def_and_call() {
        assert_eq!(
            ast("add[@a, @b] (\n    # @a + @b\n)\nlog[add[1, 2]]\nf[] (\n    #\n)"),
            "(def add (@a @b) ((# (+ @a @b))))\n(log (add 1 2))\n(def f () ((#)))\n"
        );
    }

    #[test]
    fn else_on_next_line_and_else_if() {
        let src = "\
if @a (
    log[1]
)
else if @b (
    log[2]
)
else (
    log[3]
)";
        assert_eq!(
            ast(src),
            "(if @a ((log 1)) ((if @b ((log 2)) ((log 3)))))\n"
        );
    }

    #[test]
    fn if_without_else_keeps_newline() {
        assert_eq!(ast("if @a (\n)\n\n@b = 1"), "(if @a ())\n(= @b 1)\n");
    }

    #[test]
    fn arrays_and_index() {
        assert_eq!(
            ast("@n := {1, 2}\n@n{0} = @n{1}\n@e := {}"),
            "(:= @n {1 2})\n(= ({} @n 0) ({} @n 1))\n(:= @e {})\n"
        );
        // 添字は続けて書ける。代入の左辺にも書ける
        assert_eq!(
            ast("@m{\"a\"}{0} = @m{\"b\"}{1}"),
            "(= ({} ({} @m \"a\") 0) ({} ({} @m \"b\") 1))\n"
        );
    }

    #[test]
    fn module_member_and_call() {
        assert_eq!(
            ast("@x = @u.add[1, 2] + @u.version"),
            "(= @x (+ ((. @u add) 1 2) (. @u version)))\n"
        );
        // `.名前[...]` は関数呼び出し、添字は `{ }`
        assert_eq!(
            ast("log[@u.items{0}, @a.b.f[]]"),
            "(log ({} (. @u items) 0) ((. (. @a b) f)))\n"
        );
        // 数値の小数点とは区別される
        assert_eq!(ast("@x = 1.5"), "(= @x 1.5)\n");
    }

    #[test]
    fn module_member_errors() {
        assert_eq!(
            error("@u.").message,
            "名前が必要です（ファイルの終わりがありました）"
        );
        // `@m.key = v` は `@m{"key"} = v`（モジュールかどうかは実行時に分かる）
        assert_eq!(ast("@u.count = 1"), "(= ({} @u \"count\") 1)\n");
        assert_eq!(
            ast("@u.items{0}.x = 1"),
            "(= ({} ({} ({} @u \"items\") 0) \"x\") 1)\n"
        );
        // 関数定義は名前空間つきでは書けない
        assert!(parse(&tokenize("@u.f[@a] (\n)").unwrap()).is_err());
    }

    #[test]
    fn maps() {
        assert_eq!(
            ast("@m := {\"a\": 1, @k: {:}}\n@m{\"a\"} = {}"),
            "(:= @m {\"a\": 1 @k: {:}})\n(= ({} @m \"a\") {})\n"
        );
        // { } の中の改行は無視されるので複数行に書ける
        assert_eq!(
            ast("@m := {\n    \"x\": 1,\n    \"y\": 2\n}"),
            "(:= @m {\"x\": 1 \"y\": 2})\n"
        );
    }

    #[test]
    fn mixed_array_and_map() {
        let e = error("@m := {\"a\": 1, 2}");
        assert_eq!(e.message, "`:`が必要です（`}`がありました）");
        let e = error("@a := {1, 2: 3}");
        assert_eq!(e.message, "`,` か `}`が必要です（`:`がありました）");
    }

    #[test]
    fn multiline_args() {
        assert_eq!(ast("log[\n    1,\n    2\n]"), "(log 1 2)\n");
    }

    #[test]
    fn string_interpolation() {
        assert_eq!(
            ast(r#"log["Hi, @name! \@x"]"#),
            "(log \"Hi, @name! \\@x\")\n"
        );
    }

    #[test]
    fn two_statements_on_one_line() {
        let e = error("while true ( @a = 1 @b = 2 )");
        assert_eq!(e.message, "改行が必要です（`@b`がありました）");
        assert_eq!(e.span, Span { line: 1, col: 21 });
    }

    #[test]
    fn missing_block() {
        let e = error("if @a\n");
        assert_eq!(e.message, "`(`が必要です（改行がありました）");
        assert_eq!(e.span, Span { line: 1, col: 6 });
    }

    #[test]
    fn unclosed_block() {
        let e = error("while true (\n    @a = 1\n");
        assert_eq!(e.message, "`)`が必要です（ファイルの終わりがありました）");
    }

    #[test]
    fn invalid_assign_target() {
        let e = error("1 = 2");
        assert_eq!(e.message, "代入できるのは変数か配列の要素だけです");
    }

    #[test]
    fn non_variable_param() {
        let e = error("f[1] (\n)");
        assert_eq!(e.message, "関数定義の引数は変数（`@x`）で書きます");
    }

    #[test]
    fn function_values() {
        // 関数名だけ = 関数値、無名関数、値の呼び出し
        assert_eq!(
            ast("@f := add
@g := [@a, @b] ( # @a + @b )
log[@f[1], @g[1, 2], make[][3]]"),
            "(:= @f (ref add))
(:= @g (fn (@a @b) ((# (+ @a @b)))))
(log (call @f 1) (call @g 1 2) (call (make) 3))
"
        );
        // 名前つきの呼び出しと関数定義は、これまでどおり
        assert_eq!(
            ast("f[@a] (
    # @a
)"),
            "(def f (@a) ((# @a)))
"
        );
        // 無名関数は複数行に書ける
        assert_eq!(
            ast("log[[@x] (
    @y := @x
    # @y
)]"),
            "(log (fn (@x) ((:= @y @x) (# @y))))
"
        );
        // 引数は変数だけ
        let e = error("@f := [1] ( )");
        assert_eq!(e.message, "変数（`@x`）が必要です（`1`がありました）");
    }

    #[test]
    fn for_and_nil() {
        assert_eq!(
            ast("for @x in {1, 2} (\n    log[@x]\n)\n@n := nil"),
            "(for @x {1 2} ((log @x)))\n(:= @n nil)\n"
        );
        let e = error("for 1 in @a ( )");
        assert_eq!(e.message, "変数（`@x`）が必要です（`1`がありました）");
        let e = error("for @x @a ( )");
        assert_eq!(e.message, "`in`が必要です（`@a`がありました）");
    }

    #[test]
    fn try_operator() {
        assert_eq!(
            ast("@t := file_read[@p]?\n@n := to_num[@t]? + 1\n@v := @f[1]?{0}"),
            "(:= @t (? (file_read @p)))\n(:= @n (+ (? (to_num @t)) 1))\n(:= @v ({} (? (call @f 1)) 0))\n"
        );
    }

    #[test]
    fn declare() {
        assert_eq!(ast("@x := 1 + 2"), "(:= @x (+ 1 2))\n");
        let e = error("@a{0} := 1");
        assert_eq!(e.message, "宣言できるのは変数だけです");
        let e = error("f[] := 1");
        assert_eq!(e.message, "宣言できるのは変数だけです");
    }

    #[test]
    fn chained_comparison() {
        let e = error("@x = 1 < @a < 3");
        assert!(e.message.starts_with("比較は続けて書けません"));
    }
}
