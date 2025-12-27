pub mod tokens;
use std::iter::Peekable;
use std::str::Chars;

use crate::lexer::tokens::Token;


pub struct Tokenizer<'a> {
    input: Peekable<Chars<'a>>
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable()
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace_horizontal();

        let token =match self.input.peek() {
            Some('\n') | Some('\r') => {
                self.consume_newline();
                let indent = self.count_indentation();
                Some(Token::Indent(indent))
            }
            Some(':') => {
                self.input.next();
                Some(Token::Colon)
            }
            Some('#') => {
                self.skip_comment();
                self.next_token()
            }
            Some(c) if c.is_alphanumeric() || *c == '/' || *c == '.' || *c == '_' => {
                Some(self.read_identifier())
            }
            None => Some(Token::EOF),
            _ => {
                self.input.next(); // Skip unknown
                self.next_token()
            }
        };
        token
    }

    fn read_identifier(&mut self) -> Token {
        let mut ident = String::new();
        while let Some(&c) = self.input.peek() {
            if c.is_whitespace() || c == ':' || c == '#' { break; }
            ident.push(self.input.next().unwrap());
        }
        
        match ident.as_str() {
            "server" => Token::Server,
            "location" => Token::Location,
            _ => Token::Identifier(ident),
        }
    }

    fn count_indentation(&mut self) -> usize {
        let mut count = 0;
        while let Some(&' ') = self.input.peek() {
            count += 1;
            self.input.next();
        }
        count
    }

    fn skip_whitespace_horizontal(&mut self) {
        while let Some(&c) = self.input.peek() {
            if c == ' ' || c == '\t' { self.input.next(); } 
            else { break; }
        }
    }
    
    fn consume_newline(&mut self) {
        if let Some('\n') = self.input.next() {}
    }

    fn skip_comment(&mut self) {
        while let Some(&c) = self.input.peek() {
            if c == '\n' { break; }
            self.input.next();
        }
    }
}