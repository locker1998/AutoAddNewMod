use crate::pipeline::logic::ast::{BinOp, Expr, Stmt};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    String(String),
    Number(f64),
    True,
    False,
    If,
    Elif,
    Else,
    Var,
    Field,
    Return,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Not,
    Plus,
    Minus,
    Star,
    Slash,
    Dot,
    Comma,
    Colon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Newline,
    Indent,
    Dedent,
    Eof,
}

struct Lexer<'a> {
    src: &'a [u8],
    i: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            i: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.i += 1;
        if ch == b'\n' {
            self.line += 1;
        }
        Some(ch)
    }

    fn starts_line(&self) -> bool {
        self.i == 0 || self.src.get(self.i.saturating_sub(1)) == Some(&b'\n')
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
            self.bump();
        }
    }

    fn indent_of_rest(&self) -> usize {
        let mut n = 0;
        let mut i = self.i;
        while let Some(ch) = self.src.get(i) {
            match ch {
                b' ' => n += 1,
                b'\t' => n += 4,
                _ => break,
            }
            i += 1;
        }
        n
    }

    fn rest_is_blank_or_comment(&self) -> bool {
        let mut i = self.i;
        while let Some(ch) = self.src.get(i) {
            match ch {
                b' ' | b'\t' | b'\r' => i += 1,
                b'\n' | b'#' => return true,
                _ => return false,
            }
        }
        true
    }

    fn lex(mut self) -> Result<Vec<(usize, Tok)>, String> {
        let mut out = Vec::new();
        let mut indents = vec![0usize];
        while self.peek().is_some() {
            if self.starts_line() {
                if self.rest_is_blank_or_comment() {
                    while let Some(ch) = self.peek() {
                        if ch == b'\n' {
                            self.bump();
                            break;
                        }
                        if ch == b'#' {
                            while let Some(c) = self.peek() {
                                if c == b'\n' {
                                    break;
                                }
                                self.bump();
                            }
                            continue;
                        }
                        self.bump();
                    }
                    continue;
                }
                let indent = self.indent_of_rest();
                self.skip_spaces();
                let last = *indents.last().unwrap();
                if indent > last {
                    indents.push(indent);
                    out.push((self.line, Tok::Indent));
                } else {
                    while indent < *indents.last().unwrap() {
                        indents.pop();
                        out.push((self.line, Tok::Dedent));
                    }
                    if indent != *indents.last().unwrap() {
                        return Err(format!("Line {}: inconsistent indentation", self.line));
                    }
                }
            }

            let line = self.line;
            match self.peek() {
                None => break,
                Some(b'\n') => {
                    self.bump();
                    if !matches!(out.last().map(|t| &t.1), Some(Tok::Newline)) {
                        out.push((line, Tok::Newline));
                    }
                }
                Some(b'#') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b' ' | b'\t' | b'\r') => self.skip_spaces(),
                Some(b'"' | b'\'') => out.push((line, Tok::String(self.lex_string()?))),
                Some(b'0'..=b'9') => out.push((line, Tok::Number(self.lex_number()?))),
                Some(b'|') => {
                    self.bump();
                    if self.peek() == Some(b'|') {
                        self.bump();
                        out.push((line, Tok::OrOr));
                    } else {
                        return Err(format!("Line {line}: unexpected `|`"));
                    }
                }
                Some(b'&') => {
                    self.bump();
                    if self.peek() == Some(b'&') {
                        self.bump();
                        out.push((line, Tok::AndAnd));
                    } else {
                        return Err(format!("Line {line}: unexpected `&`"));
                    }
                }
                Some(b'=') => {
                    self.bump();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        out.push((line, Tok::EqEq));
                    } else {
                        out.push((line, Tok::Eq));
                    }
                }
                Some(b'!') => {
                    self.bump();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        out.push((line, Tok::NotEq));
                    } else {
                        out.push((line, Tok::Not));
                    }
                }
                Some(b'<') => {
                    self.bump();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        out.push((line, Tok::Le));
                    } else {
                        out.push((line, Tok::Lt));
                    }
                }
                Some(b'>') => {
                    self.bump();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        out.push((line, Tok::Ge));
                    } else {
                        out.push((line, Tok::Gt));
                    }
                }
                Some(b'.') => {
                    self.bump();
                    out.push((line, Tok::Dot));
                }
                Some(b',') => {
                    self.bump();
                    out.push((line, Tok::Comma));
                }
                Some(b':') => {
                    self.bump();
                    out.push((line, Tok::Colon));
                }
                Some(b'+') => {
                    self.bump();
                    out.push((line, Tok::Plus));
                }
                Some(b'-') => {
                    self.bump();
                    out.push((line, Tok::Minus));
                }
                Some(b'*') => {
                    self.bump();
                    out.push((line, Tok::Star));
                }
                Some(b'/') => {
                    self.bump();
                    out.push((line, Tok::Slash));
                }
                Some(b'(') => {
                    self.bump();
                    out.push((line, Tok::LParen));
                }
                Some(b')') => {
                    self.bump();
                    out.push((line, Tok::RParen));
                }
                Some(b'[') => {
                    self.bump();
                    out.push((line, Tok::LBracket));
                }
                Some(b']') => {
                    self.bump();
                    out.push((line, Tok::RBracket));
                }
                Some(b'{') => {
                    self.bump();
                    out.push((line, Tok::LBrace));
                }
                Some(b'}') => {
                    self.bump();
                    out.push((line, Tok::RBrace));
                }
                Some(b'A'..=b'Z' | b'a'..=b'z' | b'_') => {
                    let ident = self.lex_ident();
                    let tok = match ident.as_str() {
                        "if" => Tok::If,
                        "elif" => Tok::Elif,
                        "else" => Tok::Else,
                        "var" => Tok::Var,
                        "field" => Tok::Field,
                        "return" => Tok::Return,
                        "true" => Tok::True,
                        "false" => Tok::False,
                        _ => Tok::Ident(ident),
                    };
                    out.push((line, tok));
                }
                Some(ch) => {
                    return Err(format!(
                        "Line {line}: unexpected character `{}`",
                        ch as char
                    ));
                }
            }
        }
        if !matches!(out.last().map(|t| &t.1), Some(Tok::Newline | Tok::Dedent | Tok::Eof) | None)
        {
            out.push((self.line, Tok::Newline));
        }
        while indents.len() > 1 {
            indents.pop();
            out.push((self.line, Tok::Dedent));
        }
        out.push((self.line, Tok::Eof));
        Ok(out)
    }

    fn lex_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == b'_' {
                s.push(ch as char);
                self.bump();
            } else {
                break;
            }
        }
        s
    }

    fn lex_number(&mut self) -> Result<f64, String> {
        let line = self.line;
        let mut s = String::new();
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            s.push(self.bump().unwrap() as char);
        }
        if self.peek() == Some(b'.')
            && matches!(self.src.get(self.i + 1), Some(b'0'..=b'9'))
        {
            s.push(self.bump().unwrap() as char);
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                s.push(self.bump().unwrap() as char);
            }
        }
        s.parse::<f64>()
            .map_err(|_| format!("Line {line}: invalid number"))
    }

    fn lex_string(&mut self) -> Result<String, String> {
        let line = self.line;
        let quote = self.bump().unwrap();
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return Err(format!("Line {line}: unterminated string")),
                Some(ch) if ch == quote => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'r') => s.push('\r'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'"') => s.push('"'),
                    Some(b'\'') => s.push('\''),
                    Some(c) => s.push(c as char),
                    None => return Err(format!("Line {line}: unterminated string")),
                },
                Some(ch) => s.push(ch as char),
            }
        }
        Ok(s)
    }
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.i).map(|t| &t.1).unwrap_or(&Tok::Eof)
    }

    fn line(&self) -> usize {
        self.toks.get(self.i).map(|t| t.0).unwrap_or(1)
    }

    fn bump(&mut self) -> Tok {
        let tok = self.peek().clone();
        if !matches!(tok, Tok::Eof) {
            self.i += 1;
        }
        tok
    }

    fn eat_newlines(&mut self) {
        while matches!(self.peek(), Tok::Newline) {
            self.bump();
        }
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        self.eat_newlines();
        while !matches!(self.peek(), Tok::Eof) {
            stmts.push(self.parse_stmt()?);
            self.eat_newlines();
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        self.eat_newlines();
        let line = self.line();
        match self.peek() {
            Tok::Field => {
                self.bump();
                let Tok::Ident(name) = self.bump() else {
                    return Err(format!("Line {line}: field needs a name"));
                };
                self.expect_newline()?;
                Ok(Stmt::Field { name, line })
            }
            Tok::Var => {
                self.bump();
                let Tok::Ident(name) = self.bump() else {
                    return Err(format!("Line {line}: var needs a name"));
                };
                if !matches!(self.peek(), Tok::Eq | Tok::EqEq) {
                    return Err(format!("Line {line}: expected `=` after var {name}"));
                }
                self.bump();
                let value = self.parse_expr()?;
                self.expect_newline()?;
                Ok(Stmt::Var { name, value, line })
            }
            Tok::Return => {
                self.bump();
                let value = if matches!(self.peek(), Tok::Newline | Tok::Dedent | Tok::Eof) {
                    Expr::Bool(true)
                } else {
                    self.parse_expr()?
                };
                self.expect_newline()?;
                Ok(Stmt::Return { value, line })
            }
            Tok::If => self.parse_if(),
            Tok::Ident(_) => {
                if matches!(self.toks.get(self.i + 1).map(|t| &t.1), Some(Tok::Eq)) {
                    let Tok::Ident(name) = self.bump() else {
                        unreachable!()
                    };
                    self.bump();
                    let value = self.parse_expr()?;
                    self.expect_newline()?;
                    Ok(Stmt::Assign { name, value, line })
                } else {
                    let expr = self.parse_expr()?;
                    self.expect_newline()?;
                    Ok(Stmt::Expr { expr, line })
                }
            }
            _ => {
                let expr = self.parse_expr()?;
                self.expect_newline()?;
                Ok(Stmt::Expr { expr, line })
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.bump();
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        let mut elifs = Vec::new();
        let mut else_body = None;
        loop {
            self.eat_newlines();
            match self.peek() {
                Tok::Elif => {
                    self.bump();
                    let c = self.parse_expr()?;
                    let b = self.parse_block()?;
                    elifs.push((c, b));
                }
                Tok::Else => {
                    self.bump();
                    else_body = Some(self.parse_block()?);
                    break;
                }
                _ => break,
            }
        }
        Ok(Stmt::If {
            cond,
            body,
            elifs,
            else_body,
            line,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        let line = self.line();
        if !matches!(self.peek(), Tok::Colon) {
            return Err(format!("Line {line}: expected `:`"));
        }
        self.bump();
        self.eat_newlines();
        if !matches!(self.peek(), Tok::Indent) {
            return Err(format!("Line {line}: expected indented block"));
        }
        self.bump();
        let mut stmts = Vec::new();
        loop {
            self.eat_newlines();
            match self.peek() {
                Tok::Dedent => {
                    self.bump();
                    break;
                }
                Tok::Eof => break,
                _ => stmts.push(self.parse_stmt()?),
            }
        }
        if stmts.is_empty() {
            return Err(format!("Line {line}: empty block"));
        }
        Ok(stmts)
    }

    fn expect_newline(&mut self) -> Result<(), String> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Dedent | Tok::Eof => Ok(()),
            other => Err(format!("Line {}: expected newline, found {other:?}", self.line())),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Tok::OrOr) {
            self.bump();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_cmp()?;
        while matches!(self.peek(), Tok::AndAnd) {
            self.bump();
            let right = self.parse_cmp()?;
            left = Expr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let left = self.parse_add()?;
        let op = match self.peek() {
            Tok::Eq | Tok::EqEq => BinOp::Eq,
            Tok::NotEq => BinOp::NotEq,
            Tok::Lt => BinOp::Lt,
            Tok::Gt => BinOp::Gt,
            Tok::Le => BinOp::Le,
            Tok::Ge => BinOp::Ge,
            _ => return Ok(left),
        };
        self.bump();
        let right = self.parse_add()?;
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.parse_mul()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Tok::Not) {
            self.bump();
            return Ok(Expr::UnaryNot(Box::new(self.parse_unary()?)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_atom()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.bump();
                    let Tok::Ident(name) = self.bump() else {
                        return Err(format!("Line {}: expected name after `.`", self.line()));
                    };
                    if matches!(self.peek(), Tok::LParen) {
                        self.bump();
                        let args = self.parse_args()?;
                        expr = Expr::Call {
                            callee: Box::new(Expr::Prop {
                                object: Box::new(expr),
                                name,
                            }),
                            args,
                        };
                    } else {
                        expr = Expr::Prop {
                            object: Box::new(expr),
                            name,
                        };
                    }
                }
                Tok::LParen => {
                    self.bump();
                    let args = self.parse_args()?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                Tok::LBracket => {
                    self.bump();
                    let index = self.parse_expr()?;
                    let end = if matches!(self.peek(), Tok::Colon) {
                        self.bump();
                        if matches!(self.peek(), Tok::RBracket) {
                            Some(Box::new(Expr::Null))
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        }
                    } else {
                        None
                    };
                    if !matches!(self.peek(), Tok::RBracket) {
                        return Err(format!("Line {}: expected `]`", self.line()));
                    }
                    self.bump();
                    expr = Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                        end,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if matches!(self.peek(), Tok::RParen) {
            self.bump();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            match self.peek() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RParen => {
                    self.bump();
                    break;
                }
                _ => return Err(format!("Line {}: expected `,` or `)`", self.line())),
            }
        }
        Ok(args)
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        let line = self.line();
        match self.bump() {
            Tok::True => Ok(Expr::Bool(true)),
            Tok::False => Ok(Expr::Bool(false)),
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::String(s) => Ok(Expr::String(s)),
            Tok::Ident(s) => Ok(Expr::Ident(s)),
            Tok::LParen => {
                let e = self.parse_expr()?;
                if !matches!(self.peek(), Tok::RParen) {
                    return Err(format!("Line {line}: expected `)`"));
                }
                self.bump();
                Ok(e)
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if matches!(self.peek(), Tok::RBracket) {
                    self.bump();
                    return Ok(Expr::List(items));
                }
                loop {
                    items.push(self.parse_expr()?);
                    match self.peek() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RBracket => {
                            self.bump();
                            break;
                        }
                        _ => return Err(format!("Line {line}: expected `,` or `]`")),
                    }
                }
                Ok(Expr::List(items))
            }
            Tok::LBrace => {
                let mut items = Vec::new();
                if matches!(self.peek(), Tok::RBrace) {
                    self.bump();
                    return Ok(Expr::Map(items));
                }
                loop {
                    let key = match self.bump() {
                        Tok::Ident(s) | Tok::String(s) => s,
                        _ => return Err(format!("Line {line}: expected object key")),
                    };
                    if !matches!(self.peek(), Tok::Colon) {
                        return Err(format!("Line {line}: expected `:` in object"));
                    }
                    self.bump();
                    let value = self.parse_expr()?;
                    items.push((key, value));
                    match self.peek() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RBrace => {
                            self.bump();
                            break;
                        }
                        _ => return Err(format!("Line {line}: expected `,` or `}}`")),
                    }
                }
                Ok(Expr::Map(items))
            }
            other => Err(format!("Line {line}: unexpected token {other:?}")),
        }
    }
}

pub fn parse(src: &str) -> Result<Vec<Stmt>, String> {
    let toks = Lexer::new(src).lex()?;
    Parser { toks, i: 0 }.parse_program()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_control_flow_and_fields() {
        let src = r#"
field a
field b
a.as_file().accept({"ext":"key"})
b.as_textarea()
if mod.get_type() == type.file:
	var f = mod.as_file()
	var e = f.ext
	if e == "rar" || e == "zip":
		var result = mod.extract(b.split("\n"))
		if result = true:
			return true
		else:
			message = "No password works."
			return false
	return true
else:
	return true
"#;
        parse(src).expect("parse");
    }
}
