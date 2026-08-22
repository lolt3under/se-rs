//! Recursive-descent parser for the awk sub-language.

use super::ast::*;
use super::lexer::{Tok, lex};
use anyhow::{Result, anyhow};

const KEYWORDS: &[&str] = &[
    "BEGIN", "END", "if", "else", "while", "for", "in", "print", "printf", "delete", "break",
    "continue", "next",
];

fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

pub fn parse_program(src: &str) -> Result<Program> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
    };
    let prog = p.program()?;
    if p.pos != p.toks.len() {
        return Err(anyhow!("awk: unexpected trailing tokens in program"));
    }
    Ok(prog)
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Tok> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(anyhow!("awk: expected {}", what))
        }
    }

    fn is_ident(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s == kw)
    }

    fn skip_semis(&mut self) {
        while self.eat(&Tok::Semi) {}
    }

    // -- program / rules ----------------------------------------------------

    fn program(&mut self) -> Result<Program> {
        let mut prog = Program::default();
        loop {
            self.skip_semis();
            match self.peek() {
                None => break,
                Some(Tok::Ident(s)) if s == "BEGIN" => {
                    self.advance();
                    let block = self.block()?;
                    prog.begin.extend(block);
                }
                Some(Tok::Ident(s)) if s == "END" => {
                    self.advance();
                    let block = self.block()?;
                    prog.end.extend(block);
                }
                _ => prog.main.push(self.statement()?),
            }
        }
        Ok(prog)
    }

    /// Parse a `{ ... }` block, returning its statements.
    fn block(&mut self) -> Result<Vec<Stmt>> {
        self.expect(&Tok::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        loop {
            self.skip_semis();
            if self.eat(&Tok::RBrace) {
                break;
            }
            if self.peek().is_none() {
                return Err(anyhow!("awk: unterminated '{{' block"));
            }
            stmts.push(self.statement()?);
        }
        Ok(stmts)
    }

    // -- statements ---------------------------------------------------------

    fn statement(&mut self) -> Result<Stmt> {
        match self.peek() {
            Some(Tok::LBrace) => Ok(Stmt::Block(self.block()?)),
            Some(Tok::Ident(s)) if s == "print" => {
                self.advance();
                Ok(Stmt::Print(self.print_args()?))
            }
            Some(Tok::Ident(s)) if s == "printf" => {
                self.advance();
                let args = self.print_args()?;
                if args.is_empty() {
                    return Err(anyhow!("awk: printf needs a format string"));
                }
                Ok(Stmt::Printf(args))
            }
            Some(Tok::Ident(s)) if s == "if" => self.if_stmt(),
            Some(Tok::Ident(s)) if s == "while" => self.while_stmt(),
            Some(Tok::Ident(s)) if s == "for" => self.for_stmt(),
            Some(Tok::Ident(s)) if s == "delete" => self.delete_stmt(),
            Some(Tok::Ident(s)) if s == "break" => {
                self.advance();
                Ok(Stmt::Break)
            }
            Some(Tok::Ident(s)) if s == "continue" => {
                self.advance();
                Ok(Stmt::Continue)
            }
            Some(Tok::Ident(s)) if s == "next" => {
                self.advance();
                Ok(Stmt::Next)
            }
            _ => Ok(Stmt::Expr(self.expr()?)),
        }
    }

    /// A comma-separated expression list for `print`/`printf`, possibly empty,
    /// terminated by `;`, `}`, or end of input.
    fn print_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        if matches!(self.peek(), None | Some(Tok::Semi) | Some(Tok::RBrace)) {
            return Ok(args);
        }
        args.push(self.expr()?);
        while self.eat(&Tok::Comma) {
            args.push(self.expr()?);
        }
        Ok(args)
    }

    fn if_stmt(&mut self) -> Result<Stmt> {
        self.advance(); // if
        self.expect(&Tok::LParen, "'(' after if")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let then = Box::new(self.statement()?);
        self.skip_semis();
        let els = if self.is_ident("else") {
            self.advance();
            Some(Box::new(self.statement()?))
        } else {
            None
        };
        Ok(Stmt::If(cond, then, els))
    }

    fn while_stmt(&mut self) -> Result<Stmt> {
        self.advance(); // while
        self.expect(&Tok::LParen, "'(' after while")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::While(cond, body))
    }

    fn for_stmt(&mut self) -> Result<Stmt> {
        self.advance(); // for
        self.expect(&Tok::LParen, "'(' after for")?;
        // for (k in arr) — detect `Ident in Ident )`
        let for_in_var = match self.peek().cloned() {
            Some(Tok::Ident(var))
                if !is_keyword(&var)
                    && self.toks.get(self.pos + 1) == Some(&Tok::Ident("in".into())) =>
            {
                Some(var)
            }
            _ => None,
        };
        if let Some(var) = for_in_var {
            self.advance(); // var
            self.advance(); // in
            let array = self.ident_name("array name after 'in'")?;
            self.expect(&Tok::RParen, "')'")?;
            let body = Box::new(self.statement()?);
            return Ok(Stmt::ForIn { var, array, body });
        }
        // C-style: for (init; cond; post)
        let init = if self.peek() == Some(&Tok::Semi) {
            None
        } else {
            Some(Box::new(self.statement()?))
        };
        self.expect(&Tok::Semi, "';' in for")?;
        let cond = if self.peek() == Some(&Tok::Semi) {
            None
        } else {
            Some(self.expr()?)
        };
        self.expect(&Tok::Semi, "';' in for")?;
        let post = if self.peek() == Some(&Tok::RParen) {
            None
        } else {
            Some(Box::new(self.statement()?))
        };
        self.expect(&Tok::RParen, "')'")?;
        let body = Box::new(self.statement()?);
        Ok(Stmt::For {
            init,
            cond,
            post,
            body,
        })
    }

    fn delete_stmt(&mut self) -> Result<Stmt> {
        self.advance(); // delete
        let array = self.ident_name("array name after 'delete'")?;
        let indices = if self.eat(&Tok::LBracket) {
            let mut idx = vec![self.expr()?];
            while self.eat(&Tok::Comma) {
                idx.push(self.expr()?);
            }
            self.expect(&Tok::RBracket, "']'")?;
            idx
        } else {
            Vec::new()
        };
        Ok(Stmt::Delete { array, indices })
    }

    fn ident_name(&mut self, what: &str) -> Result<String> {
        match self.advance() {
            Some(Tok::Ident(s)) if !is_keyword(s) => Ok(s.clone()),
            _ => Err(anyhow!("awk: expected {}", what)),
        }
    }

    // -- expressions (precedence climbing) ---------------------------------

    fn expr(&mut self) -> Result<Expr> {
        self.assign()
    }

    fn assign(&mut self) -> Result<Expr> {
        let lhs = self.ternary()?;
        let op = match self.peek() {
            Some(Tok::Assign) => Some(AssignOp::Set),
            Some(Tok::AddAssign) => Some(AssignOp::Add),
            Some(Tok::SubAssign) => Some(AssignOp::Sub),
            Some(Tok::MulAssign) => Some(AssignOp::Mul),
            Some(Tok::DivAssign) => Some(AssignOp::Div),
            Some(Tok::ModAssign) => Some(AssignOp::Mod),
            Some(Tok::PowAssign) => Some(AssignOp::Pow),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let lv = to_lvalue(lhs)?;
            let rhs = self.assign()?;
            Ok(Expr::Assign(op, lv, Box::new(rhs)))
        } else {
            Ok(lhs)
        }
    }

    fn ternary(&mut self) -> Result<Expr> {
        let cond = self.or()?;
        if self.eat(&Tok::Question) {
            let a = self.assign()?;
            self.expect(&Tok::Colon, "':' in ?:")?;
            let b = self.assign()?;
            Ok(Expr::Ternary(Box::new(cond), Box::new(a), Box::new(b)))
        } else {
            Ok(cond)
        }
    }

    fn or(&mut self) -> Result<Expr> {
        let mut left = self.and()?;
        while self.eat(&Tok::Or) {
            let right = self.and()?;
            left = Expr::Logical(LogOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut left = self.in_expr()?;
        while self.eat(&Tok::And) {
            let right = self.in_expr()?;
            left = Expr::Logical(LogOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn in_expr(&mut self) -> Result<Expr> {
        let mut left = self.comparison()?;
        while self.is_ident("in") {
            self.advance();
            let array = self.ident_name("array name after 'in'")?;
            left = Expr::In(Box::new(left), array);
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Expr> {
        let left = self.concat()?;
        let op = match self.peek() {
            Some(Tok::Lt) => Some(BinOp::Lt),
            Some(Tok::Le) => Some(BinOp::Le),
            Some(Tok::Gt) => Some(BinOp::Gt),
            Some(Tok::Ge) => Some(BinOp::Ge),
            Some(Tok::Eq) => Some(BinOp::Eq),
            Some(Tok::Ne) => Some(BinOp::Ne),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.concat()?;
            Ok(Expr::Binary(op, Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn concat(&mut self) -> Result<Expr> {
        let mut left = self.additive()?;
        while self.starts_value() {
            let right = self.additive()?;
            left = Expr::Concat(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// Whether the next token can begin a value-expression — used to detect awk
    /// concatenation. `+`/`-` are excluded (additive already consumed them) so
    /// `a - b` is subtraction, not `a` concat `-b`.
    fn starts_value(&self) -> bool {
        match self.peek() {
            Some(Tok::Num(_)) | Some(Tok::Str(_)) | Some(Tok::Dollar) | Some(Tok::LParen)
            | Some(Tok::Not) | Some(Tok::Incr) | Some(Tok::Decr) => true,
            Some(Tok::Ident(s)) => !is_keyword(s),
            _ => false,
        }
    }

    fn additive(&mut self) -> Result<Expr> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.multiplicative()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Tok::Not) => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)))
            }
            Some(Tok::Minus) => {
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)))
            }
            Some(Tok::Plus) => {
                self.advance();
                Ok(Expr::Unary(UnOp::Pos, Box::new(self.unary()?)))
            }
            Some(Tok::Incr) => {
                self.advance();
                let lv = to_lvalue(self.unary()?)?;
                Ok(Expr::Incr {
                    lvalue: lv,
                    delta: 1.0,
                    pre: true,
                })
            }
            Some(Tok::Decr) => {
                self.advance();
                let lv = to_lvalue(self.unary()?)?;
                Ok(Expr::Incr {
                    lvalue: lv,
                    delta: -1.0,
                    pre: true,
                })
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Expr> {
        let base = self.postfix()?;
        if self.eat(&Tok::Caret) {
            // right-associative; exponent may itself be unary (`2^-3`)
            let exp = self.unary()?;
            Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn postfix(&mut self) -> Result<Expr> {
        let e = self.primary()?;
        match self.peek() {
            Some(Tok::Incr) => {
                if let Ok(lv) = to_lvalue(e.clone()) {
                    self.advance();
                    return Ok(Expr::Incr {
                        lvalue: lv,
                        delta: 1.0,
                        pre: false,
                    });
                }
                Ok(e)
            }
            Some(Tok::Decr) => {
                if let Ok(lv) = to_lvalue(e.clone()) {
                    self.advance();
                    return Ok(Expr::Incr {
                        lvalue: lv,
                        delta: -1.0,
                        pre: false,
                    });
                }
                Ok(e)
            }
            _ => Ok(e),
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.advance();
                Ok(Expr::Num(n))
            }
            Some(Tok::Str(s)) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Some(Tok::Dollar) => {
                self.advance();
                let operand = self.primary()?;
                Ok(Expr::Field(Box::new(operand)))
            }
            Some(Tok::LParen) => {
                self.advance();
                let e = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if is_keyword(&name) {
                    return Err(anyhow!("awk: unexpected keyword '{}' in expression", name));
                }
                self.advance();
                match self.peek() {
                    Some(Tok::LParen) => {
                        self.advance();
                        let mut args = Vec::new();
                        if self.peek() != Some(&Tok::RParen) {
                            args.push(self.expr()?);
                            while self.eat(&Tok::Comma) {
                                args.push(self.expr()?);
                            }
                        }
                        self.expect(&Tok::RParen, "')'")?;
                        Ok(Expr::Call(name, args))
                    }
                    Some(Tok::LBracket) => {
                        self.advance();
                        let mut idx = vec![self.expr()?];
                        while self.eat(&Tok::Comma) {
                            idx.push(self.expr()?);
                        }
                        self.expect(&Tok::RBracket, "']'")?;
                        Ok(Expr::Index(name, idx))
                    }
                    _ => Ok(Expr::Var(name)),
                }
            }
            other => Err(anyhow!("awk: unexpected token {:?} in expression", other)),
        }
    }
}

fn to_lvalue(e: Expr) -> Result<LValue> {
    match e {
        Expr::Var(name) => Ok(LValue::Var(name)),
        Expr::Field(idx) => Ok(LValue::Field(idx)),
        Expr::Index(name, idx) => Ok(LValue::Index(name, idx)),
        _ => Err(anyhow!("awk: invalid assignment target")),
    }
}
