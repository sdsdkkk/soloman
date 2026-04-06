//! Recursive-descent parser.

use crate::ast::{BinOp, Expr, Program, Stmt};
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    i: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, i: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.i]
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.i].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.i += 1;
        }
        t
    }

    fn expect_semi(&mut self) -> Result<(), String> {
        match &self.peek().kind {
            TokenKind::Semi => {
                self.bump();
                Ok(())
            }
            _ => Err(format!(
                "{}:{}: expected ';', found {:?}",
                self.peek().line,
                self.peek().col,
                self.peek().kind
            )),
        }
    }

    pub fn parse_program(mut self) -> Result<Program, String> {
        let mut stmts = Vec::new();
        while !matches!(self.peek().kind, TokenKind::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        Ok(Program { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.peek().line;
        let col = self.peek().col;
        if let TokenKind::Ident(name) = &self.peek().kind.clone() {
            let name = name.clone();
            if matches!(self.tokens.get(self.i + 1).map(|t| &t.kind), Some(TokenKind::Eq)) {
                self.bump();
                self.bump(); // =
                let value = self.parse_expr()?;
                self.expect_semi()?;
                return Ok(Stmt::Assign { name, value });
            }
        }
        let e = self.parse_expr()?;
        self.expect_semi()
            .map_err(|_| format!("{}:{}: expected ';' after statement", line, col))?;
        Ok(Stmt::Expr(e))
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_add()
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_mul()?;
        loop {
            match &self.peek().kind {
                TokenKind::Plus => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                TokenKind::Minus => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary {
                        op: BinOp::Sub,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            match &self.peek().kind {
                TokenKind::Star => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::Binary {
                        op: BinOp::Mul,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                TokenKind::Slash => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::Binary {
                        op: BinOp::Div,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek().kind, TokenKind::Minus) {
            self.bump();
            let inner = self.parse_unary()?;
            return Ok(Expr::Binary {
                op: BinOp::Sub,
                left: Box::new(Expr::Int(0)),
                right: Box::new(inner),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let t = self.peek().clone();
        match t.kind {
            TokenKind::Int(n) => {
                self.bump();
                Ok(Expr::Int(n))
            }
            TokenKind::Str(ref s) => {
                let s = s.clone();
                self.bump();
                Ok(Expr::Str(s))
            }
            TokenKind::Ident(name) => {
                self.bump();
                if matches!(self.peek().kind, TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek().kind, TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek().kind, TokenKind::Comma) {
                            self.bump();
                            args.push(self.parse_expr()?);
                        }
                    }
                    match &self.peek().kind {
                        TokenKind::RParen => {
                            self.bump();
                            Ok(Expr::Call { name, args })
                        }
                        _ => Err(format!(
                            "{}:{}: expected ')' after arguments",
                            self.peek().line,
                            self.peek().col
                        )),
                    }
                } else {
                    Ok(Expr::Var(name))
                }
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                match &self.peek().kind {
                    TokenKind::RParen => {
                        self.bump();
                        Ok(inner)
                    }
                    _ => Err(format!(
                        "{}:{}: expected ')'",
                        self.peek().line,
                        self.peek().col
                    )),
                }
            }
            _ => Err(format!(
                "{}:{}: unexpected token {:?}",
                t.line, t.col, t.kind
            )),
        }
    }
}
