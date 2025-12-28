pub mod tokens;
use std::iter::Peekable;
use std::str::Chars;

use crate::lexer::tokens::{Loc, Token, TokenType};

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input: input.chars().peekable(), line: 1, col: 1 }
    }

    fn advance(&mut self) {
        if let Some(c) = self.input.next() {
            if c == '\n' { self.line += 1; self.col = 1; } 
            else { self.col += 1; }
        }
    }

    fn peek(&mut self) -> Option<&char> { self.input.peek() }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        let mut is_start_of_line = true;

        while let Some(&c) = self.peek() {
            let loc = Loc { line: self.line, col: self.col };

            // 1. Handle Indentation (Only at start of line)
            if is_start_of_line && c.is_whitespace() && c != '\n' {
                let mut spaces = 0;
                while let Some(&w) = self.peek() {
                    if w == ' ' { spaces += 1; self.advance(); }
                    else if w == '\t' { spaces += 4; self.advance(); } // Soft tab
                    else { break; }
                }
                // Emit indent only if relevant content follows
                if let Some(&next) = self.peek() {
                    if next != '\n' && next != '#' {
                        tokens.push(Token { kind: TokenType::Indent(spaces), loc });
                    }
                }
                continue;
            }

            // 2. Skip Comments
            if c == '#' {
                while let Some(&n) = self.peek() {
                    if n == '\n' { break; }
                    self.advance();
                }
                continue;
            }

            match c {
                // 3. Structural Symbols
                ':' => { tokens.push(Token { kind: TokenType::Colon, loc }); self.advance(); is_start_of_line = false; }
                '-' => { tokens.push(Token { kind: TokenType::Dash, loc }); self.advance(); is_start_of_line = false; }
                '[' => { tokens.push(Token { kind: TokenType::LBracket, loc }); self.advance(); is_start_of_line = false; }
                ']' => { tokens.push(Token { kind: TokenType::RBracket, loc }); self.advance(); is_start_of_line = false; }
                ',' => { tokens.push(Token { kind: TokenType::Comma, loc }); self.advance(); is_start_of_line = false; }
                
                // 4. Newline (Reset start_of_line)
                '\n' => { 
                    tokens.push(Token { kind: TokenType::Newline, loc }); 
                    self.advance(); 
                    is_start_of_line = true; 
                }

                // 5. Quoted Strings "host"
                '"' => {
                    self.advance(); // consume opening quote
                    let mut val = String::new();
                    while let Some(&next_c) = self.peek() {
                        if next_c == '"' { self.advance(); break; }
                        val.push(next_c);
                        self.advance();
                    }
                    tokens.push(Token { kind: TokenType::StringLit(val), loc });
                    is_start_of_line = false;
                }

                // 6. Generic Whitespace (Skip it! This fixes "ports :")
                c if c.is_whitespace() => { self.advance(); }

                // 7. Text/Numbers
                _ => {
                    let mut val = String::new();
                    while let Some(&n) = self.peek() {
                        // Allow dots and slashes in text (e.g. 127.0.0.1 or /bin/bash)
                        if n.is_alphanumeric() || "._-/".contains(n) {
                            val.push(n);
                            self.advance();
                        } else { break; }
                    }
                    if let Ok(num) = val.parse::<u64>() {
                        tokens.push(Token { kind: TokenType::Number(num), loc });
                    } else {
                        tokens.push(Token { kind: TokenType::Text(val), loc });
                    }
                    is_start_of_line = false;
                }
            }
        }
        Ok(tokens)
    }
}