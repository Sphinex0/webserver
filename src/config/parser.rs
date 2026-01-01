use std::{collections::HashMap, fmt};
use crate::lexer::tokens::{Loc, Token, TokenType};

// --- Error Handling ---

#[derive(Debug)]
pub struct ConfigError {
    pub message: String,
    pub loc: Option<Loc>,
    pub context: Vec<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "❌ \x1b[1;31mConfiguration Error\x1b[0m: {}", self.message)?;
        if let Some(loc) = self.loc {
            write!(f, " \x1b[38;5;244m(at line {}, col {})\x1b[0m", loc.line, loc.col)?;
        }
        if !self.context.is_empty() {
            writeln!(f, "\n   \x1b[1;34mContext trace:\x1b[0m")?;
            for (i, ctx) in self.context.iter().rev().enumerate() {
                let indent = " ".repeat(2 + i * 2);
                writeln!(f, "{}↳ {}", indent, ctx)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {} 

pub type ParseResult<T> = Result<T, ConfigError>;

// --- Config Parser ---

pub struct ConfigParser {
    pub tokens: Vec<Token>,
    pub cursor: usize,
}

impl ConfigParser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
    }

    pub fn peek_kind(&self) -> Option<&TokenType> {
        self.tokens.get(self.cursor).map(|t| &t.kind)
    }

    pub fn peek_kind_at(&self, offset: usize) -> Option<&TokenType> {
        self.tokens.get(self.cursor + offset).map(|t| &t.kind)
    }

    pub fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    pub fn peek_loc(&self) -> Option<Loc> {
        self.tokens.get(self.cursor).map(|t| t.loc)
    }

    pub fn next_token(&mut self) -> Option<&Token> {
        if self.cursor < self.tokens.len() {
            let t = &self.tokens[self.cursor];
            self.cursor += 1;
            Some(t)
        } else {
            None
        }
    }

    pub fn consume(&mut self, expected: TokenType) -> ParseResult<()> {
        let loc = self.peek_loc();
        match self.next_token() {
            Some(t) if std::mem::discriminant(&t.kind) == std::mem::discriminant(&expected) => Ok(()),
            Some(t) => Err(ConfigError {
                message: format!("Expected {:?}, found {:?}", expected, t.kind),
                loc: Some(t.loc),
                context: Vec::new(),
            }),
            None => Err(ConfigError {
                message: format!("Expected {:?}, found EOF", expected),
                loc,
                context: Vec::new(),
            }),
        }
    }

    pub fn consume_key(&mut self, _key: &str) -> ParseResult<()> {
        self.cursor += 1; // consume text
        self.consume(TokenType::Colon)
    }

    pub fn skip_newlines(&mut self) {
        while let Some(k) = self.peek_kind() {
            if matches!(k, TokenType::Newline | TokenType::Indent(_)) {
                self.cursor += 1;
            } else {
                break;
            }
        }
    }

    pub fn skip_newlines_only(&mut self) -> bool {
        let mut skipped = false;
        while let Some(TokenType::Newline) = self.peek_kind() {
            self.cursor += 1;
            skipped = true;
        }
        skipped
    }

    pub fn parse_scalar_string(&mut self) -> ParseResult<String> {
        let loc = self.peek_loc();
        match self.next_token() {
            Some(t) => match &t.kind {
                TokenType::Text(s) | TokenType::StringLit(s) => Ok(s.clone()),
                _ => Err(ConfigError {
                    message: format!("Expected string, found {:?}", t.kind),
                    loc: Some(t.loc),
                    context: Vec::new(),
                }),
            },
            None => Err(ConfigError {
                message: "Expected string, found EOF".to_string(),
                loc,
                context: Vec::new(),
            }),
        }
    }

    pub fn parse_scalar_number(&mut self) -> ParseResult<u64> {
        let loc = self.peek_loc();
        match self.next_token() {
            Some(t) => match t.kind {
                TokenType::Number(n) => Ok(n),
                _ => Err(ConfigError {
                    message: format!("Expected number, found {:?}", t.kind),
                    loc: Some(t.loc),
                    context: Vec::new(),
                }),
            },
            None => Err(ConfigError {
                message: "Expected number, found EOF".to_string(),
                loc,
                context: Vec::new(),
            }),
        }
    }

    pub fn skip_value(&mut self, min_indent: usize) -> ParseResult<()> {
        loop {
            if matches!(self.peek_kind(), Some(TokenType::Newline)) {
                break;
            }
            if self.peek_kind().is_none() {
                return Ok(())
            }
            self.cursor += 1;
        }

        loop {
            if matches!(self.peek_kind(), Some(TokenType::Newline)) {
                self.cursor += 1; // Consume Newline

                let indent_val = if let Some(TokenType::Indent(n)) = self.peek_kind() {
                    Some(*n)
                } else {
                    None
                };

                if let Some(n) = indent_val {
                    if n > min_indent {
                        self.cursor += 1; // Consume Indent
                        loop {
                            if matches!(self.peek_kind(), Some(TokenType::Newline)) {
                                break;
                            }
                            if self.peek_kind().is_none() {
                                return Ok(())
                            }
                            self.cursor += 1;
                        }
                    } else {
                        return Ok(())
                    }
                } else {
                    if matches!(self.peek_kind(), Some(TokenType::Newline)) {
                        continue;
                    }
                    return Ok(())
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    // --- Helpers for Macro ---

    /// Checks for indentation compliance. Returns true if struct parsing should continue, false if block ended.
    pub fn check_indentation(&mut self, min_indent: usize, struct_indent: &mut Option<usize>) -> ParseResult<bool> {
        self.skip_newlines_only();
        
        if let Some(TokenType::Indent(n)) = self.peek_kind() {
            let indent = *n;
            if indent < min_indent { return Ok(false); } // Dedent -> End of block
            
            // Check for list item start
            if let Some(TokenType::Dash) = self.peek_kind_at(1) {
                return Ok(false); // Dash at this level means end of struct, start of next list item
            }

            if let Some(current) = *struct_indent {
                if indent != current {
                    if indent < current {
                        if indent > min_indent {
                             return Err(ConfigError {
                                message: format!("Indentation mismatch: found {} < current {} but > parent {}", indent, current, min_indent),
                                loc: self.peek_loc(),
                                context: vec![],
                            });
                        }
                        return Ok(false);
                    } else {
                         return Err(ConfigError {
                             message: format!("Indentation mismatch: found {} > current {}", indent, current),
                             loc: self.peek_loc(),
                             context: vec![],
                         });
                    }
                }
            } else {
                if indent <= min_indent && min_indent > 0 { return Ok(false); }
                *struct_indent = Some(indent);
            }
            self.cursor += 1; // Consume indent
        }
        Ok(true)
    }

    pub fn is_end_of_block(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenType::Dash))
    }

    pub fn parse_map_key(&self) -> ParseResult<Option<String>> {
        match self.peek_kind() {
            Some(TokenType::Text(s)) | Some(TokenType::StringLit(s)) => {
                if let Some(TokenType::Colon) = self.peek_kind_at(1) {
                    Ok(Some(s.clone()))
                } else {
                    Err(ConfigError {
                        message: format!("Expected key-value pair, found scalar '{}'", s),
                        loc: self.peek_loc(),
                        context: vec![],
                    })
                }
            },
            Some(TokenType::Number(n)) => {
                Err(ConfigError {
                    message: format!("Expected map key, found number '{}'", n),
                    loc: self.peek_loc(),
                    context: vec![],
                })
            },
            None => Ok(None),
            _ => Ok(None), // Other tokens imply end of block or error caught later
        }
    }
}

// --- Dynamic Parser Trait ---

pub trait FromYaml: Sized {
    fn from_yaml(parser: &mut ConfigParser, min_indent: usize) -> ParseResult<Self>;

    fn from_str(input: &str) -> ParseResult<Self> {
        let mut lexer = crate::lexer::Lexer::new(input);
        let tokens = lexer.tokenize().map_err(|e| ConfigError {
            message: e,
            loc: None,
            context: vec!["Lexing phase".to_string()],
        })?;
        let mut parser = ConfigParser::new(tokens);
        let result = Self::from_yaml(&mut parser, 0)?;
        
        parser.skip_newlines();
        if parser.peek_kind().is_some() {
            return Err(ConfigError {
                message: format!("Unexpected content after configuration: {:?}", parser.peek_kind().unwrap()),
                loc: parser.peek_loc(),
                context: vec![],
            });
        }
        Ok(result)
    }
}

// --- Primitive Implementations ---

impl FromYaml for String {
    fn from_yaml(parser: &mut ConfigParser, _min_indent: usize) -> ParseResult<Self> {
        parser.parse_scalar_string()
    }
}

impl FromYaml for u16 {
    fn from_yaml(parser: &mut ConfigParser, _min_indent: usize) -> ParseResult<Self> {
        let loc = parser.peek_loc();
        let n = parser.parse_scalar_number()?;
        if n > u16::MAX as u64 {
            return Err(ConfigError {
                message: format!("Value {} is out of range for u16 (max {})", n, u16::MAX),
                loc,
                context: vec![],
            });
        }
        Ok(n as u16)
    }
}

impl FromYaml for usize {
    fn from_yaml(parser: &mut ConfigParser, _min_indent: usize) -> ParseResult<Self> {
        parser.parse_scalar_number().map(|n| n as usize)
    }
}

impl FromYaml for bool {
    fn from_yaml(parser: &mut ConfigParser, _min_indent: usize) -> ParseResult<Self> {
        let val = parser.parse_scalar_string()?;
        Ok(val == "true" || val == "on")
    }
}

impl<T: FromYaml> FromYaml for Option<T> {
    fn from_yaml(parser: &mut ConfigParser, min_indent: usize) -> ParseResult<Self> {
        Ok(Some(T::from_yaml(parser, min_indent)?))
    }
}

impl<T: FromYaml> FromYaml for Vec<T> {
    fn from_yaml(parser: &mut ConfigParser, min_indent: usize) -> ParseResult<Self> {
        let mut items = Vec::new();
        let skipped_newline = parser.skip_newlines_only();

        if let Some(TokenType::LBracket) = parser.peek_kind() {
            parser.consume(TokenType::LBracket)?;
            loop {
                while matches!(
                    parser.peek_kind(),
                    Some(TokenType::Newline) | Some(TokenType::Indent(_))
                ) {
                    parser.cursor += 1;
                }
                if let Some(TokenType::RBracket) = parser.peek_kind() {
                    parser.consume(TokenType::RBracket)?;
                    break;
                }
                items.push(T::from_yaml(parser, min_indent)?);
                while matches!(
                    parser.peek_kind(),
                    Some(TokenType::Newline) | Some(TokenType::Indent(_))
                ) {
                    parser.cursor += 1;
                }
                if let Some(TokenType::Comma) = parser.peek_kind() {
                    parser.consume(TokenType::Comma)?;
                }
            }
        } else {
            let mut list_indent = 0;
            if let Some(TokenType::Indent(n)) = parser.peek_kind() {
                list_indent = *n;
                if list_indent < min_indent {
                    return Ok(items);
                }
            }

            match parser.peek_kind() {
                Some(TokenType::Dash) => {
                    if !skipped_newline {
                        return Err(ConfigError {
                            message: "Block list item must start on a new line".to_string(),
                            loc: parser.peek_loc(),
                            context: vec![],
                        });
                    }
                }
                Some(TokenType::Indent(_)) | Some(TokenType::Newline) | None => {} // Continue parsing
                _ => {
                    return Err(ConfigError {
                        message: format!(
                            "Expected list (starting with '[' or '-'), found {:?}",
                            parser.peek_kind().unwrap()
                        ),
                        loc: parser.peek_loc(),
                        context: vec![],
                    });
                }
            }

            loop {
                let newline_skipped_in_loop = parser.skip_newlines_only();
                if let Some(TokenType::Indent(n)) = parser.peek_kind() {
                    if *n < list_indent {
                        break;
                    }

                    if *n > list_indent {
                        if let Some(TokenType::Dash) = parser.peek_kind_at(1) {
                            return Err(ConfigError {
                                message: format!(
                                    "Indentation mismatch in list: found {}, expected {}",
                                    *n, list_indent
                                ),
                                loc: parser.peek_loc(),
                                context: vec![],
                            });
                        }
                    }

                    parser.cursor += 1;
                } else {
                    if !matches!(parser.peek_kind(), Some(TokenType::Dash)) {
                        if list_indent > 0 { break; }
                    }
                }

                if let Some(TokenType::Dash) = parser.peek_kind() {
                    if list_indent == 0 && !newline_skipped_in_loop {
                        return Err(ConfigError {
                            message: "Block list item must start on a new line".to_string(),
                            loc: parser.peek_loc(),
                            context: vec![],
                        });
                    }

                    parser.consume(TokenType::Dash)?;
                    items.push(T::from_yaml(parser, list_indent)?);
                } else {
                    break;
                }
            }
        }
        Ok(items)
    }
}

impl<K, V> FromYaml for HashMap<K, V>
where
    K: FromYaml + std::cmp::Eq + std::hash::Hash + fmt::Display,
    V: FromYaml,
{
    fn from_yaml(parser: &mut ConfigParser, _min_indent: usize) -> ParseResult<Self> {
        let mut map = HashMap::new();
        parser.skip_newlines_only();

        let mut map_indent = 0;
        if let Some(TokenType::Indent(n)) = parser.peek_kind() {
            map_indent = *n;
        }

        loop {
            parser.skip_newlines_only();
            if let Some(TokenType::Indent(n)) = parser.peek_kind() {
                if *n < map_indent {
                    break;
                }
                parser.cursor += 1; // Consume indent
            } else if map_indent > 0 {
                break;
            }

            match parser.peek_kind() {
                None | Some(TokenType::Dash) | Some(TokenType::RBracket) => break,
                _ => {} // Continue parsing
            }

            let key = K::from_yaml(parser, map_indent).map_err(|mut e| {
                e.context.push("parsing map key".to_string());
                e
            })?;

            parser.consume(TokenType::Colon)?;

            if map.contains_key(&key) {
                return Err(ConfigError {
                    message: format!("Duplicate key '{}' in map", key),
                    loc: parser.peek_loc(),
                    context: vec![],
                });
            }

            let value = V::from_yaml(parser, map_indent).map_err(|mut e| {
                e.context
                    .push(format!("parsing map value for key '{}'", key));
                e
            })?;

            map.insert(key, value);
        }
        Ok(map)
    }
}
