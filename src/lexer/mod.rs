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

            // 1. Handle Indentation at start of line
            if is_start_of_line && c != '\n' {
                let mut spaces = 0;
                if c.is_whitespace() {
                    while let Some(&w) = self.peek() {
                        if w == ' ' { spaces += 1; self.advance(); }
                        else if w == '\t' { spaces += 4; self.advance(); } // Soft tab
                        else { break; }
                    }
                }
                
                // Emit indent only if relevant content follows
                if let Some(&next) = self.peek() {
                    if next != '\n' && next != '#' {
                        tokens.push(Token { kind: TokenType::Indent(spaces), loc });
                    }
                }
                is_start_of_line = false;
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
                '-' => { 
                    self.advance(); // Consume the dash first
                    
                    // Now peek checks the next character
                    let next_is_separator = match self.peek() {
                        Some(n) => n.is_whitespace(), 
                        None => true, 
                    };

                    if next_is_separator {
                        tokens.push(Token { kind: TokenType::Dash, loc }); 
                        is_start_of_line = false; 
                    } else {
                        // Treat as text start
                        let mut val = String::from("-");
                        
                        while let Some(&n) = self.peek() {
                            if n.is_alphanumeric() || "._-/".contains(n) {
                                val.push(n);
                                self.advance();
                            } else { break; }
                        }
                        tokens.push(Token { kind: TokenType::Text(val), loc });
                        is_start_of_line = false;
                    }
                }
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
                    
                    if val.is_empty() {
                        // Unknown character (e.g. '%', '@', etc.)
                        // Advance to prevent infinite loop and optionally return error
                        let char_opt = self.peek().copied(); 
                        if let Some(c) = char_opt {
                             return Err(format!("Unexpected character: '{}' at line {}, col {}", c, self.line, self.col));
                        } else {
                             break; // EOF
                        }
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