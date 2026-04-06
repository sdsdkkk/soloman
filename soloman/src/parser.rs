//! Recursive-descent parser.

use crate::ast::{BinOp, Expr, FnDef, Item, ObjDef, Param, Program, Stmt, Ty};
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
        let mut items = Vec::new();
        let mut stmts = Vec::new();
        while !matches!(self.peek().kind, TokenKind::Eof) {
            match self.peek().kind {
                TokenKind::KwImport => items.push(self.parse_import()?),
                TokenKind::KwFn => items.push(Item::Function(self.parse_fn_def()?)),
                TokenKind::KwObject => items.push(Item::Object(self.parse_object_def()?)),
                _ => stmts.push(self.parse_stmt()?),
            }
        }
        Ok(Program { items, stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.peek().line;
        let col = self.peek().col;
        match self.peek().kind {
            TokenKind::KwLet => return self.parse_let_stmt(),
            TokenKind::KwReturn => return self.parse_return_stmt(),
            _ => {}
        }
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

    fn parse_import(&mut self) -> Result<Item, String> {
        let t = self.bump(); // import
        let TokenKind::KwImport = t.kind else {
            return Err("internal".to_string());
        };
        let path = match self.bump().kind {
            TokenKind::Str(s) => s,
            other => {
                return Err(format!(
                    "{}:{}: expected string after import, found {:?}",
                    self.peek().line,
                    self.peek().col,
                    other
                ));
            }
        };
        self.expect_semi()?;
        Ok(Item::Import(path))
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, String> {
        let kw = self.bump(); // fn
        let TokenKind::KwFn = kw.kind else {
            return Err("internal".to_string());
        };
        let name = match self.bump().kind {
            TokenKind::Ident(s) => s,
            other => {
                return Err(format!(
                    "{}:{}: expected function name, found {:?}",
                    kw.line, kw.col, other
                ));
            }
        };
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokenKind::RParen) {
            loop {
                let pname = match self.bump().kind {
                    TokenKind::Ident(s) => s,
                    other => {
                        return Err(format!(
                            "{}:{}: expected parameter name, found {:?}",
                            self.peek().line,
                            self.peek().col,
                            other
                        ));
                    }
                };
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                params.push(Param { name: pname, ty });
                if matches!(self.peek().kind, TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Arrow)?;
        let ret = self.parse_type()?;
        self.expect(TokenKind::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace) {
            if matches!(self.peek().kind, TokenKind::Eof) {
                return Err("unexpected EOF in function body".to_string());
            }
            body.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(FnDef {
            name,
            params,
            ret,
            body,
        })
    }

    fn parse_object_def(&mut self) -> Result<ObjDef, String> {
        let kw = self.bump(); // object
        let TokenKind::KwObject = kw.kind else {
            return Err("internal".to_string());
        };
        let name = match self.bump().kind {
            TokenKind::Ident(s) => s,
            other => {
                return Err(format!(
                    "{}:{}: expected object name, found {:?}",
                    kw.line, kw.col, other
                ));
            }
        };
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace) {
            let fname = match self.bump().kind {
                TokenKind::Ident(s) => s,
                other => {
                    return Err(format!(
                        "{}:{}: expected field name, found {:?}",
                        self.peek().line,
                        self.peek().col,
                        other
                    ));
                }
            };
            self.expect(TokenKind::Colon)?;
            let fty = self.parse_type()?;
            self.expect_semi()?;
            fields.push((fname, fty));
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ObjDef { name, fields })
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, String> {
        let kw = self.bump(); // let
        let TokenKind::KwLet = kw.kind else {
            return Err("internal".to_string());
        };
        let name = match self.bump().kind {
            TokenKind::Ident(s) => s,
            other => {
                return Err(format!(
                    "{}:{}: expected identifier after let, found {:?}",
                    kw.line, kw.col, other
                ));
            }
        };
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        self.expect(TokenKind::Eq)?;
        let value = self.parse_expr()?;
        self.expect_semi()?;
        Ok(Stmt::Let { name, ty, value })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, String> {
        let kw = self.bump(); // return
        let TokenKind::KwReturn = kw.kind else {
            return Err("internal".to_string());
        };
        if matches!(self.peek().kind, TokenKind::Semi) {
            self.bump();
            return Ok(Stmt::Return(None));
        }
        let e = self.parse_expr()?;
        self.expect_semi()?;
        Ok(Stmt::Return(Some(e)))
    }

    fn parse_type(&mut self) -> Result<Ty, String> {
        match self.bump().kind {
            TokenKind::Ident(s) if s == "Int" => Ok(Ty::Int),
            TokenKind::Ident(s) if s == "Str" => Ok(Ty::Str),
            TokenKind::Ident(s) if s == "Unit" => Ok(Ty::Unit),
            TokenKind::Ident(s) => Ok(Ty::Object(s)),
            other => Err(format!(
                "{}:{}: expected type name, found {:?}",
                self.peek().line,
                self.peek().col,
                other
            )),
        }
    }

    fn expect(&mut self, k: TokenKind) -> Result<(), String> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(&k) {
            self.bump();
            Ok(())
        } else {
            Err(format!(
                "{}:{}: expected {:?}, found {:?}",
                self.peek().line,
                self.peek().col,
                k,
                self.peek().kind
            ))
        }
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
        let mut base = match t.kind {
            TokenKind::Int(n) => {
                self.bump();
                Expr::Int(n)
            }
            TokenKind::Str(ref s) => {
                let s = s.clone();
                self.bump();
                Expr::Str(s)
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
                            Expr::Call { name, args }
                        }
                        _ => Err(format!(
                            "{}:{}: expected ')' after arguments",
                            self.peek().line,
                            self.peek().col
                        ))?,
                    }
                } else if matches!(self.peek().kind, TokenKind::LBrace) {
                    // Object literal: Name{ field: expr, ... }
                    self.bump();
                    let mut fields = Vec::new();
                    if !matches!(self.peek().kind, TokenKind::RBrace) {
                        loop {
                            let fname = match self.bump().kind {
                                TokenKind::Ident(s) => s,
                                other => {
                                    return Err(format!(
                                        "{}:{}: expected field name, found {:?}",
                                        self.peek().line,
                                        self.peek().col,
                                        other
                                    ));
                                }
                            };
                            self.expect(TokenKind::Colon)?;
                            let v = self.parse_expr()?;
                            fields.push((fname, v));
                            if matches!(self.peek().kind, TokenKind::Comma) {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(TokenKind::RBrace)?;
                    Expr::ObjLit {
                        object: name,
                        fields,
                    }
                } else {
                    Expr::Var(name)
                }
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr()?;
                match &self.peek().kind {
                    TokenKind::RParen => {
                        self.bump();
                        inner
                    }
                    _ => Err(format!(
                        "{}:{}: expected ')'",
                        self.peek().line,
                        self.peek().col
                    ))?,
                }
            }
            _ => Err(format!(
                "{}:{}: unexpected token {:?}",
                t.line, t.col, t.kind
            ))?,
        };

        // postfix field access: expr.ident
        while matches!(self.peek().kind, TokenKind::Dot) {
            self.bump();
            let name = match self.bump().kind {
                TokenKind::Ident(s) => s,
                other => {
                    return Err(format!(
                        "{}:{}: expected field name after '.', found {:?}",
                        self.peek().line,
                        self.peek().col,
                        other
                    ));
                }
            };
            base = Expr::Field {
                base: Box::new(base),
                name,
            };
        }
        Ok(base)
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expr, Item, Stmt, Ty};
    use crate::lexer::tokenize;

    use super::Parser;

    fn parse(src: &str) -> crate::ast::Program {
        let toks = tokenize(src).expect("tokenize");
        Parser::new(toks).parse_program().expect("parse")
    }

    #[test]
    fn parses_import_function_object_and_toplevel_stmt() {
        let p = parse(
            r#"
            import "mods/a.sol";
            object Point { x: Int; y: Int; }
            fn add(a: Int, b: Int) -> Int { return a + b; }
            let n: Int = add(1, 2);
            "#,
        );
        assert_eq!(p.items.len(), 3);
        assert_eq!(p.stmts.len(), 1);
        assert!(matches!(p.items[0], Item::Import(_)));
        assert!(matches!(p.items[1], Item::Object(_)));
        assert!(matches!(p.items[2], Item::Function(_)));
        assert!(matches!(p.stmts[0], Stmt::Let { .. }));
    }

    #[test]
    fn parses_object_literal_and_field_access() {
        let p = parse(
            r#"
            object P { x: Int; }
            fn getx(p: P) -> Int { return p.x; }
            let p: P = P{ x: 42 };
            "#,
        );
        let Item::Function(f) = &p.items[1] else {
            panic!("expected fn");
        };
        assert_eq!(f.params[0].ty, Ty::Object("P".to_string()));
        match &f.body[0] {
            Stmt::Return(Some(Expr::Field { .. })) => {}
            other => panic!("unexpected body stmt: {other:?}"),
        }
    }

    #[test]
    fn parser_errors_for_missing_semi() {
        let toks = tokenize("let x: Int = 1").expect("tokenize");
        let err = Parser::new(toks).parse_program().unwrap_err();
        assert!(err.contains("expected ';'"));
    }
}
