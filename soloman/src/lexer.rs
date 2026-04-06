//! Lexical analysis for `.sol` sources.

use std::iter::Peekable;
use std::str::CharIndices;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // literals / identifiers
    Ident(String),
    Int(i64),
    Str(String),

    // keywords
    KwFn,
    KwLet,
    KwReturn,
    KwImport,
    KwObject,

    // operators / punctuation
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Colon,
    Dot,
    Arrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    src: &'a str,
    it: Peekable<CharIndices<'a>>,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            it: src.char_indices().peekable(),
            line: 1,
            col: 1,
        }
    }

    fn bump(&mut self) -> Option<(usize, char)> {
        let n = self.it.next()?;
        if n.1 == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(n)
    }

    fn peek_char(&mut self) -> Option<char> {
        self.it.peek().map(|(_, c)| *c)
    }

    fn byte_offset(&mut self) -> usize {
        self.it.peek().map(|(i, _)| *i).unwrap_or(self.src.len())
    }

    fn skip_ws(&mut self) {
        loop {
            while self.peek_char().is_some_and(|c| c.is_whitespace()) {
                self.bump();
            }
            let rest = &self.src[self.byte_offset()..];
            if rest.starts_with("//") {
                self.bump();
                self.bump();
                while self.peek_char().is_some_and(|c| c != '\n') {
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    fn lex_string(&mut self, start_line: usize, start_col: usize) -> Result<Token, String> {
        self.bump(); // opening "
        let mut out = String::new();
        loop {
            match self.peek_char() {
                None => return Err(format!("{}:{}: unterminated string", start_line, start_col)),
                Some('"') => {
                    self.bump();
                    return Ok(Token {
                        kind: TokenKind::Str(out),
                        line: start_line,
                        col: start_col,
                    });
                }
                Some('\n') => {
                    return Err(format!("{}:{}: newline in string", start_line, start_col));
                }
                Some('\\') => {
                    self.bump();
                    match self.bump() {
                        Some((_, esc)) => match esc {
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            '\\' => out.push('\\'),
                            '"' => out.push('"'),
                            _ => {
                                return Err(format!(
                                    "{}:{}: unknown escape \\{}",
                                    self.line, self.col, esc
                                ));
                            }
                        },
                        None => {
                            return Err(format!("{}:{}: unterminated string", start_line, start_col));
                        }
                    }
                }
                Some(c) => {
                    self.bump();
                    out.push(c);
                }
            }
        }
    }

    fn lex_number(&mut self, start: usize, start_line: usize, start_col: usize) -> Token {
        let mut end = start;
        while let Some(&(i, c)) = self.it.peek() {
            if c.is_ascii_digit() {
                end = i + c.len_utf8();
                self.bump();
            } else {
                break;
            }
        }
        let slice = &self.src[start..end];
        let n: i64 = slice.parse().unwrap_or(0);
        Token {
            kind: TokenKind::Int(n),
            line: start_line,
            col: start_col,
        }
    }

    fn lex_ident(&mut self, start: usize, start_line: usize, start_col: usize) -> Token {
        let mut end = start;
        while let Some(&(_, c)) = self.it.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                if let Some((i, ch)) = self.bump() {
                    end = i + ch.len_utf8();
                }
            } else {
                break;
            }
        }
        let name = self.src[start..end].to_string();
        let kind = match name.as_str() {
            "fn" => TokenKind::KwFn,
            "let" => TokenKind::KwLet,
            "return" => TokenKind::KwReturn,
            "import" => TokenKind::KwImport,
            "object" => TokenKind::KwObject,
            _ => TokenKind::Ident(name),
        };
        Token {
            kind,
            line: start_line,
            col: start_col,
        }
    }

    pub fn next_token(&mut self) -> Result<Token, String> {
        self.skip_ws();
        let start_line = self.line;
        let start_col = self.col;
        let Some((idx, c)) = self.it.peek().copied() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                line: start_line,
                col: start_col,
            });
        };

        match c {
            '"' => self.lex_string(start_line, start_col),
            '0'..='9' => Ok(self.lex_number(idx, start_line, start_col)),
            'a'..='z' | 'A'..='Z' | '_' => Ok(self.lex_ident(idx, start_line, start_col)),
            '+' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Plus,
                    line: start_line,
                    col: start_col,
                })
            }
            '-' => {
                self.bump();
                if matches!(self.peek_char(), Some('>')) {
                    self.bump();
                    return Ok(Token {
                        kind: TokenKind::Arrow,
                        line: start_line,
                        col: start_col,
                    });
                }
                Ok(Token {
                    kind: TokenKind::Minus,
                    line: start_line,
                    col: start_col,
                })
            }
            '*' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Star,
                    line: start_line,
                    col: start_col,
                })
            }
            '/' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Slash,
                    line: start_line,
                    col: start_col,
                })
            }
            '=' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Eq,
                    line: start_line,
                    col: start_col,
                })
            }
            ':' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Colon,
                    line: start_line,
                    col: start_col,
                })
            }
            '.' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Dot,
                    line: start_line,
                    col: start_col,
                })
            }
            '(' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::LParen,
                    line: start_line,
                    col: start_col,
                })
            }
            ')' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::RParen,
                    line: start_line,
                    col: start_col,
                })
            }
            '{' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::LBrace,
                    line: start_line,
                    col: start_col,
                })
            }
            '}' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::RBrace,
                    line: start_line,
                    col: start_col,
                })
            }
            ',' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Comma,
                    line: start_line,
                    col: start_col,
                })
            }
            ';' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Semi,
                    line: start_line,
                    col: start_col,
                })
            }
            _ => Err(format!(
                "{}:{}: unexpected character {:?}",
                start_line, start_col, c
            )),
        }
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut lex = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        let t = lex.next_token()?;
        let eof = matches!(t.kind, TokenKind::Eof);
        out.push(t);
        if eof {
            break;
        }
    }
    Ok(out)
}
