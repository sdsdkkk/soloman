use std::env;
use std::fs;

// ======================
// TOKEN
// ======================

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Int(i64),
    Plus,
    Minus,
    Mul,
    Div,
    LParen,
    RParen,
    Print,
    Semi,
    EOF,
}

// ======================
// LEXER
// ======================

struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(input: String) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn integer(&mut self) -> i64 {
        let start = self.pos;

        while let Some(c) = self.current() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        self.input[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .unwrap()
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        match self.current() {
            Some(c) if c.is_ascii_digit() => Token::Int(self.integer()),

            Some('+') => {
                self.advance();
                Token::Plus
            }

            Some('-') => {
                self.advance();
                Token::Minus
            }

            Some('*') => {
                self.advance();
                Token::Mul
            }

            Some('/') => {
                self.advance();
                Token::Div
            }

            Some('(') => {
                self.advance();
                Token::LParen
            }

            Some(')') => {
                self.advance();
                Token::RParen
            }

            Some(';') => {
                self.advance();
                Token::Semi
            }

            Some('p') => {
                let remaining: String =
                    self.input[self.pos..].iter().collect();

                if remaining.starts_with("print") {
                    self.pos += 5;
                    Token::Print
                } else {
                    panic!("Unexpected token");
                }
            }

            None => Token::EOF,

            _ => panic!("Invalid character"),
        }
    }
}

// ======================
// AST
// ======================

enum Expr {
    Num(i64),
    BinOp(Box<Expr>, Token, Box<Expr>),
}

enum Stmt {
    Print(Expr),
}

struct Program {
    stmts: Vec<Stmt>,
}

// ======================
// PARSER
// ======================

struct Parser {
    lexer: Lexer,
    current: Token,
}

impl Parser {
    fn new(mut lexer: Lexer) -> Self {
        let current = lexer.next_token();
        Self { lexer, current }
    }

    fn eat(&mut self, expected: Token) {
        if std::mem::discriminant(&self.current)
            == std::mem::discriminant(&expected)
        {
            self.current = self.lexer.next_token();
        } else {
            panic!("Unexpected token");
        }
    }

    fn factor(&mut self) -> Expr {
        match self.current.clone() {
            Token::Int(n) => {
                self.eat(Token::Int(0));
                Expr::Num(n)
            }

            Token::LParen => {
                self.eat(Token::LParen);
                let expr = self.expr();
                self.eat(Token::RParen);
                expr
            }

            _ => panic!("Expected number"),
        }
    }

    fn term(&mut self) -> Expr {
        let mut node = self.factor();

        loop {
            match self.current {
                Token::Mul => {
                    self.eat(Token::Mul);
                    node = Expr::BinOp(
                        Box::new(node),
                        Token::Mul,
                        Box::new(self.factor()),
                    );
                }

                Token::Div => {
                    self.eat(Token::Div);
                    node = Expr::BinOp(
                        Box::new(node),
                        Token::Div,
                        Box::new(self.factor()),
                    );
                }

                _ => break,
            }
        }

        node
    }

    fn expr(&mut self) -> Expr {
        let mut node = self.term();

        loop {
            match self.current {
                Token::Plus => {
                    self.eat(Token::Plus);
                    node = Expr::BinOp(
                        Box::new(node),
                        Token::Plus,
                        Box::new(self.term()),
                    );
                }

                Token::Minus => {
                    self.eat(Token::Minus);
                    node = Expr::BinOp(
                        Box::new(node),
                        Token::Minus,
                        Box::new(self.term()),
                    );
                }

                _ => break,
            }
        }

        node
    }

    fn statement(&mut self) -> Stmt {
        self.eat(Token::Print);
        let expr = self.expr();
        self.eat(Token::Semi);
        Stmt::Print(expr)
    }

    fn program(&mut self) -> Program {
        let mut stmts = Vec::new();

        while self.current != Token::EOF {
            stmts.push(self.statement());
        }

        Program { stmts }
    }
}

// ======================
// CODEGEN
// ======================

struct CodeGen {
    output: String,
}

impl CodeGen {
    fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn gen_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Num(n) => {
                self.emit(&format!("    mov rax, {}", n));
            }

            Expr::BinOp(left, op, right) => {
                self.gen_expr(left);
                self.emit("    push rax");

                self.gen_expr(right);
                self.emit("    pop rbx");

                match op {
                    Token::Plus =>
                        self.emit("    add rax, rbx"),

                    Token::Minus => {
                        self.emit("    sub rbx, rax");
                        self.emit("    mov rax, rbx");
                    }

                    Token::Mul =>
                        self.emit("    imul rax, rbx"),

                    Token::Div => {
                        self.emit("    mov rdx, 0");
                        self.emit("    mov rcx, rax");
                        self.emit("    mov rax, rbx");
                        self.emit("    idiv rcx");
                    }

                    _ => {}
                }
            }
        }
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Print(expr) => {
                self.gen_expr(expr);
                self.emit("    mov rdi, rax");
                self.emit("    call print_int");
            }
        }
    }

    fn generate(mut self, program: Program) -> String {

        self.emit("global _start");
        self.emit("section .text");

        self.emit("print_int:");
        self.emit("    mov rcx, buffer+20");
        self.emit("    mov rbx, 10");
        self.emit("    mov rax, rdi");

        self.emit("convert:");
        self.emit("    xor rdx, rdx");
        self.emit("    div rbx");
        self.emit("    add dl, '0'");
        self.emit("    dec rcx");
        self.emit("    mov [rcx], dl");
        self.emit("    test rax, rax");
        self.emit("    jnz convert");

        self.emit("    mov rax, 1");
        self.emit("    mov rdi, 1");
        self.emit("    mov rsi, rcx");
        self.emit("    mov rdx, buffer+20");
        self.emit("    sub rdx, rcx");
        self.emit("    syscall");

        self.emit("    mov rax, 1");
        self.emit("    mov rdi, 1");
        self.emit("    mov rsi, newline");
        self.emit("    mov rdx, 1");
        self.emit("    syscall");

        self.emit("    ret");

        self.emit("_start:");

        for stmt in program.stmts {
            self.gen_stmt(&stmt);
        }

        self.emit("    mov rax, 60");
        self.emit("    xor rdi, rdi");
        self.emit("    syscall");

        self.emit("section .bss");
        self.emit("buffer resb 21");

        self.emit("section .data");
        self.emit("newline db 10");

        self.output
    }
}

// ======================
// MAIN
// ======================

fn main() {

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: compiler <file>");
        return;
    }

    let source = fs::read_to_string(&args[1]).unwrap();

    let lexer = Lexer::new(source);
    let mut parser = Parser::new(lexer);

    let program = parser.program();

    let asm = CodeGen::new().generate(program);

    fs::write("out.asm", asm).unwrap();

    println!("Compiled to out.asm");
}
