use std::{collections::HashMap, fmt};
use derive_yaml::FromYaml;

use crate::lexer::tokens::{Loc, Token, TokenType};

// --- Constants ---
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_SERVER_NAME: &str = "_";
const DEFAULT_MAX_BODY_SIZE: usize = 1_048_576; // 1MB
const DEFAULT_ROUTE_PATH: &str = "/";
const DEFAULT_ROOT: &str = "./www";
const DEFAULT_FILE: &str = "index.html";

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

type ParseResult<T> = Result<T, ConfigError>;

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
        Self::from_yaml(&mut parser, 0)
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
        parser.parse_scalar_number().map(|n| n as u16)
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
        parser.skip_newlines_only();

        if let Some(TokenType::LBracket) = parser.peek_kind() {
            parser.consume(TokenType::LBracket)?;
            loop {
                while matches!(parser.peek_kind(), Some(TokenType::Newline) | Some(TokenType::Indent(_))) {
                    parser.cursor += 1;
                }
                if let Some(TokenType::RBracket) = parser.peek_kind() {
                    parser.consume(TokenType::RBracket)?;
                    break;
                }
                items.push(T::from_yaml(parser, min_indent)?);
                while matches!(parser.peek_kind(), Some(TokenType::Newline) | Some(TokenType::Indent(_))) {
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
                if list_indent < min_indent { return Ok(items); }
            }

            loop {
                parser.skip_newlines_only();
                if let Some(TokenType::Indent(n)) = parser.peek_kind() {
                    if *n < list_indent { break; }
                    
                    // STRICT CHECK:
                    // If we see an indent greater than list_indent, it usually means 
                    // it belongs to the previous item (e.g. multi-line string or nested object field),
                    // NOT a new list item.
                    // HOWEVER, we are looking for the next item which starts with a Dash.
                    // If we see Indent(>n) then Dash, that's an indented list item which is mismatched.
                    // If we see Indent(>n) then Key/Value, that's content of previous item.
                    
                    // But here, Vec<T> is iterating over items.
                    // We expect the next token (after optional indent) to be Dash.
                    // If indent > list_indent, we need to check if it's followed by Dash.
                    // If yes -> Error (inconsistent indent).
                    // If no -> It's probably part of previous item, but Vec<T> loop shouldn't be parsing inside item content?
                    // actually T::from_yaml consumes the item. So when we are back here, we expect the NEXT item.
                    // So any content here MUST start with a Dash at `list_indent`.
                    
                    if *n > list_indent {
                         // Check if this is a start of a new item (Dash) or just debris
                         if let Some(TokenType::Dash) = parser.peek_kind_at(1) {
                             return Err(ConfigError {
                                message: format!("Indentation mismatch in list: found {}, expected {}", *n, list_indent),
                                loc: parser.peek_loc(),
                                context: vec![],
                             });
                         }
                         // If it's not a dash, it might be something else. 
                         // But for now, let's strictly enforce list item start.
                         // Actually, if T consumes everything properly, we should only see:
                         // 1. Same indent + Dash (next item)
                         // 2. Less indent (end of list)
                         // 3. Same indent + something else (invalid list item start)
                         
                         // If we see greater indent here, it means T didn't consume everything, 
                         // OR the file structure is broken.
                         // Let's treat it as an error for strictness if it looks like a list item.
                         // But simpler: just enforce equality if we see a dash.
                    }
                    
                    parser.cursor += 1;
                } else if list_indent > 0 { break; }

                if let Some(TokenType::Dash) = parser.peek_kind() {
                     // Double check: if we didn't have an indent token (list_indent=0), 
                     // but min_indent > 0, that's handled by first check.
                     // But if list_indent > 0, we must have consumed it above.
                     // If we are here, we are ready to consume Dash.
                    parser.consume(TokenType::Dash)?;
                    items.push(T::from_yaml(parser, list_indent)?);
                } else { break; }
            }
        }
        Ok(items)
    }
}


impl<K, V>
    FromYaml for HashMap<K, V>
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
                if *n < map_indent { break; }
                parser.cursor += 1;
            } else if map_indent > 0 { break; }

            match parser.peek_kind() {
                 None | Some(TokenType::Dash) | Some(TokenType::RBracket) => break,
                 _ => {}
            }

            let key = K::from_yaml(parser, map_indent)
                .map_err(|mut e| { e.context.push("parsing map key".to_string()); e })?;
            
            parser.consume(TokenType::Colon)?;
            
            let value = V::from_yaml(parser, map_indent)
                .map_err(|mut e| { e.context.push(format!("parsing map value for key '{}'", key)); e })?;
            
            map.insert(key, value);
        }
        Ok(map)
    }
}

// --- Struct Definitions ---

#[derive(Debug, Clone, FromYaml)]
pub struct RouteConfig {
    pub path: String,
    pub methods: Vec<String>,
    pub redirection: Option<String>,
    pub root: String,
    pub default_file: String,
    pub cgi_ext: Option<String>,
    pub autoindex: bool,
    pub client_max_body_size: usize,
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            path: DEFAULT_ROUTE_PATH.to_string(),
            methods: vec!["GET".to_string(), "HEAD".to_string()],
            redirection: None,
            root: DEFAULT_ROOT.to_string(),
            default_file: DEFAULT_FILE.to_string(),
            cgi_ext: None,
            autoindex: false,
            client_max_body_size: DEFAULT_MAX_BODY_SIZE,
        }
    }
}

#[derive(Debug, Clone, FromYaml)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, FromYaml)]
pub struct ServerConfig {
    pub host: String,
    pub ports: Vec<u16>,
    pub server_name: String,
    pub default_server: bool,
    pub error_pages: HashMap<u16, String>,
    pub client_max_body_size: usize,
    pub routes: Vec<RouteConfig>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            ports: vec![DEFAULT_PORT],
            server_name: DEFAULT_SERVER_NAME.to_string(),
            default_server: false,
            error_pages: HashMap::new(),
            client_max_body_size: DEFAULT_MAX_BODY_SIZE,
            routes: Vec::new(),
        }
    }
}

impl ServerConfig {
    pub fn find_route(&self, path: &str) -> Option<&RouteConfig> {
        let mut best_match: Option<(&String, &RouteConfig)> = None;
        for route in &self.routes {
            if path.starts_with(&route.path) {
                match best_match {
                    None => best_match = Some((&route.path, route)),
                    Some((best_prefix, _)) => {
                        if route.path.len() > best_prefix.len() {
                            best_match = Some((&route.path, route));
                        }
                    }
                }
            }
        }
        best_match.map(|(_, route)| route)
    }
}

// --- Config Parser ---

pub struct ConfigParser {
    pub tokens: Vec<Token>,
    pub cursor: usize,
}

impl ConfigParser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            cursor: 0,
        }
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
            } else { break; }
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
                 return Ok(());
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
                                 return Ok(());
                             }
                            self.cursor += 1;
                        }
                    } else {
                        return Ok(());
                    }
                } else {
                    // check if another newline (empty line)
                    if matches!(self.peek_kind(), Some(TokenType::Newline)) {
                        continue;
                    }
                    return Ok(());
                }
            } else {
                break;
            }
        }
        Ok(())
    }
}

// --- Display Implementations ---

impl fmt::Display for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m")?;
        writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mNetwork:\x1b[0m     \x1b[32m{}\\x1b[0m \x1b[38;5;244mvia ports\x1b[0m \x1b[1;32m{:?}\x1b[0m", self.host, self.ports)?;
        writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mIdentity:\x1b[0m    \x1b[36m{}\x1b[0m", self.server_name)?;
        writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mDefault:\x1b[0m     \x1b[{}m{}\x1b[0m", if self.default_server { "32" } else { "31" }, if self.default_server { "YES" } else { "NO" })?;
        writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mBody Limit:\x1b[0m  \x1b[33m{} KB\x1b[0m", self.client_max_body_size / 1024)?;

        if !self.error_pages.is_empty() {
            writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mError Pages:\x1b[0m")?;
            for (code, path) in &self.error_pages {
                writeln!(f, "    \x1b[38;5;244m{:4}\x1b[0m → \x1b[31m{}\x1b[0m", code, path)?;
            }
        }

        writeln!(f, "\n  \x1b[1;37m📋 ROUTING TABLE ({}) \x1b[0m", self.routes.len())?;
        writeln!(f, "  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m")?;

        let mut sorted_routes = self.routes.clone();
        sorted_routes.sort_by(|a, b| a.path.cmp(&b.path));

        for (idx, route) in sorted_routes.iter().enumerate() {
            let is_last = idx == sorted_routes.len() - 1;
            let branch = if is_last { "  └──" } else { "  ├──" };
            writeln!(f, "  \x1b[38;5;244m{}\\x1b[0m \x1b[1;37m{}\x1b[0m", branch, route.path)?;
            route.fmt_details(f, is_last)?;
            if !is_last { writeln!(f, "  \x1b[38;5;244m    │\x1b[0m")?; }
        }
        Ok(())
    }
}

impl RouteConfig {
    fn fmt_details(&self, f: &mut fmt::Formatter<'_>, is_last_route: bool) -> fmt::Result {
        let indent = if is_last_route { "     " } else { "  │  " };
        let methods_fmt = self.methods.join(" | ");
        let route_limit = format!("{} KB", self.client_max_body_size / 1024);

        writeln!(f, "  \x1b[38;5;250m{}├─ Methods:\x1b[0m \x1b[48;5;236m\x1b[38;5;250m {}\x1b[0m", if is_last_route { "   " } else { "    " }, methods_fmt)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Root:\x1b[0m    \x1b[32m{}\x1b[0m", indent, self.root)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Default:\x1b[0m  \x1b[36m{}\x1b[0m", indent, self.default_file)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Body Limit:\x1b[0m \x1b[33m{}\x1b[0m", indent, route_limit)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Autoindex:\x1b[0m \x1b[{}m{}\x1b[0m", indent, if self.autoindex { "32" } else { "31" }, if self.autoindex { "ON" } else { "OFF" })?;

        if let Some(redir) = &self.redirection {
            writeln!(f, "  \x1b[38;5;250m{}├─ Redirect:\x1b[0m \x1b[35m{}\x1b[0m", indent, redir)?;
        }
        if let Some(cgi) = &self.cgi_ext {
            writeln!(f, "  \x1b[38;5;250m{}└─ CGI:\x1b[0m     \x1b[38;5;208m{}\x1b[0m", indent, cgi)?;
        } else {
            writeln!(f, "  \x1b[38;5;250m{}└─ CGI:\x1b[0m      \x1b[31mDISABLED\x1b[0m", indent)?;
        }
        Ok(())
    }
}

pub fn display_config(configs: &Vec<ServerConfig>) {
    println!("\n\x1b[1;35m 🌐 SERVER CONFIGURATION DASHBOARD\x1b[0m");
    println!("\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");
    for (i, server) in configs.iter().enumerate() {
        println!("\n  \x1b[1;37mSERVER BLOCK {:02}\x1b[0m", i + 1);
        print!("{}", server);
    }
    println!("\n\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");
    println!(" \x1b[1;32m✔\x1b[0m Configuration loaded successfully - Ready for requests!\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_parsing() {
        let path = "config.yaml";
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path).unwrap();
            let config = Config::from_str(&content).expect("Failed to parse config");
            assert!(config.servers.len() >= 1);
            assert_eq!(config.servers[0].host, "127.0.0.1");
            assert!(config.servers[0].ports.len() > 0);
        }
    }
}
