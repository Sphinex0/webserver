use std::{collections::HashMap, fmt, iter::Peekable};

use crate::lexer::tokens::{Token, TokenType};

// --- Constants ---
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_SERVER_NAME: &str = "_";
const DEFAULT_MAX_BODY_SIZE: usize = 1_048_576; // 1MB
const DEFAULT_ROUTE_PATH: &str = "/";
const DEFAULT_ROOT: &str = "./www";
const DEFAULT_FILE: &str = "index.html";

type ParseResult<T> = Result<T, String>;

// --- Configuration Structs ---

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub ports: Vec<u16>,
    pub server_name: String,
    pub default_server: bool,
    pub error_pages: HashMap<u16, String>,
    pub client_max_body_size: usize,
    pub routes: HashMap<String, RouteConfig>,
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
            routes: HashMap::new(),
        }
    }
}

impl ServerConfig {
    /// Finds the best matching route for a given request path.
    /// Matches are based on the longest matching prefix.
    pub fn find_route(&self, path: &str) -> Option<&RouteConfig> {
        let mut best_match: Option<(&String, &RouteConfig)> = None;
        for (prefix, route) in &self.routes {
            if path.starts_with(prefix) {
                match best_match {
                    None => best_match = Some((prefix, route)),
                    Some((best_prefix, _)) => {
                        if prefix.len() > best_prefix.len() {
                            best_match = Some((prefix, route));
                        }
                    }
                }
            }
        }
        best_match.map(|(_, route)| route)
    }
}

// --- Display Implementations ---

impl fmt::Display for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m")?;
        writeln!(
            f,
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mNetwork:\x1b[0m     \x1b[32m{}\x1b[0m \x1b[38;5;244mvia ports\x1b[0m \x1b[1;32m{:?}\x1b[0m",
            self.host, self.ports
        )?;
        writeln!(
            f,
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mIdentity:\x1b[0m    \x1b[36m{}\x1b[0m",
            self.server_name
        )?;
        writeln!(
            f,
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mDefault:\x1b[0m     \x1b[{}m{}\x1b[0m",
            if self.default_server { "32" } else { "31" },
            if self.default_server { "YES" } else { "NO" }
        )?;
        writeln!(
            f,
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mBody Limit:\x1b[0m  \x1b[33m{} KB\x1b[0m",
            self.client_max_body_size / 1024
        )?;

        if !self.error_pages.is_empty() {
            writeln!(f, "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mError Pages:\x1b[0m")?;
            for (code, path) in &self.error_pages {
                writeln!(f, "    \x1b[38;5;244m{:4}\x1b[0m → \x1b[31m{}\x1b[0m", code, path)?;
            }
        }

        writeln!(f, "\n  \x1b[1;37m📋 ROUTING TABLE ({}) \x1b[0m", self.routes.len())?;
        writeln!(f, "  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m")?;

        let mut sorted_routes: Vec<_> = self.routes.iter().collect();
        sorted_routes.sort_by(|a, b| a.0.cmp(b.0));

        for (idx, (path, route)) in sorted_routes.iter().enumerate() {
            let is_last = idx == sorted_routes.len() - 1;
            let branch = if is_last { "  └──" } else { "  ├──" };
            
            // Delegate detail formatting to RouteConfig if desired, or inline here
            writeln!(f, "  \x1b[38;5;244m{}\x1b[0m \x1b[1;37m{}\x1b[0m", branch, path)?;
            route.fmt_details(f, is_last)?;
            
            if !is_last {
                writeln!(f, "  \x1b[38;5;244m    │\x1b[0m")?;
            }
        }
        Ok(())
    }
}

impl RouteConfig {
    // Helper to print details with correct indentation based on tree position
    fn fmt_details(&self, f: &mut fmt::Formatter<'_>, is_last_route: bool) -> fmt::Result {
        let indent = if is_last_route { "     " } else { "  │  " };
        let methods_fmt = self.methods.join(" | ");
        let route_limit = format!("{} KB", self.client_max_body_size / 1024);

        writeln!(
            f,
            "  \x1b[38;5;250m{}├─ Methods:\x1b[0m \x1b[48;5;236m\x1b[38;5;250m {} \x1b[0m",
            if is_last_route { "   " } else { "    " }, methods_fmt
        )?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Root:\x1b[0m    \x1b[32m{}\x1b[0m", indent, self.root)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Default:\x1b[0m  \x1b[36m{}\x1b[0m", indent, self.default_file)?;
        writeln!(f, "  \x1b[38;5;250m{}├─ Body Limit:\x1b[0m \x1b[33m{}\x1b[0m", indent, route_limit)?;
        writeln!(
            f,
            "  \x1b[38;5;250m{}├─ Autoindex:\x1b[0m \x1b[{}m{}\x1b[0m",
            indent,
            if self.autoindex { "32" } else { "31" },
            if self.autoindex { "ON" } else { "OFF" }
        )?;

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

// --- Config Parser ---

pub struct ConfigParser {
    tokens: Peekable<std::vec::IntoIter<Token>>,
}

impl ConfigParser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
        }
    }

    // --- Helper Methods ---

    fn peek_kind(&mut self) -> Option<&TokenType> {
        self.tokens.peek().map(|t| &t.kind)
    }

    fn consume(&mut self, expected: TokenType) -> Result<(), String> {
        match self.tokens.next() {
            Some(t) if std::mem::discriminant(&t.kind) == std::mem::discriminant(&expected) => {
                Ok(())
            }
            Some(t) => Err(format!(
                "Expected {:?}, found {:?} at line {}",
                expected, t.kind, t.loc.line
            )),
            None => Err(format!("Expected {:?}, found EOF", expected)),
        }
    }

    fn skip_newlines(&mut self) {
        while let Some(k) = self.peek_kind() {
            if matches!(k, TokenType::Newline | TokenType::Indent(_)) {
                self.tokens.next();
            } else {
                break;
            }
        }
    }

    fn skip_newlines_only(&mut self) {
        while let Some(TokenType::Newline) = self.peek_kind() {
            self.tokens.next();
        }
    }

    // --- Entry Point ---

    pub fn parse(&mut self) -> ParseResult<Vec<ServerConfig>> {
        self.skip_newlines();

        match self.tokens.next() {
            Some(t) if matches!(t.kind, TokenType::Text(ref s) if s == "servers") => {}
            _ => return Err("Config must start with 'servers:'".to_string()),
        }
        self.consume(TokenType::Colon)?;

        let mut servers = Vec::new();
        self.skip_newlines();

        while let Some(TokenType::Dash) = self.peek_kind() {
            self.consume(TokenType::Dash)?;
            servers.push(self.parse_server_block()?);
            self.skip_newlines();
        }

        Ok(servers)
    }

    // --- Server Parsing ---

    fn parse_server_block(&mut self) -> ParseResult<ServerConfig> {
        let mut config = ServerConfig::default();

        loop {
            self.skip_newlines();
            
            match self.peek_kind() {
                Some(TokenType::Dash) | None => break,
                Some(TokenType::Indent(_)) => {
                    self.tokens.next();
                    continue;
                } 
                _ => {}
            }

            let key = match self.tokens.next() {
                Some(t) => match t.kind {
                    TokenType::Text(s) | TokenType::StringLit(s) => s,
                    _ => return Err(format!("Expected Key, found {:?} at line {}", t.kind, t.loc.line)),
                },
                None => break,
            };

            self.consume(TokenType::Colon)?;

            match key.as_str() {
                "host" => config.host = self.parse_string()?,
                "ports" => config.ports = self.parse_list(|p| p.parse_number().map(|n| n as u16))?,
                "server_name" => config.server_name = self.parse_string()?,
                "default_server" => {
                    let val = self.parse_string()?;
                    config.default_server = val == "true" || val == "on";
                },
                "client_max_body_size" => config.client_max_body_size = self.parse_number()? as usize,
                "error_pages" => config.error_pages = self.parse_error_pages()?,
                "routes" => {
                    let routes_list = self.parse_route_list()?;
                    for route in routes_list {
                        config.routes.insert(route.path.clone(), route);
                    }
                }
                _ => self.skip_unknown_value(),
            }
        }

        Ok(config)
    }

    fn skip_unknown_value(&mut self) {
        if let Some(t) = self.tokens.peek() {
            match t.kind {
                TokenType::Text(_) | TokenType::Number(_) | TokenType::StringLit(_) => {
                    self.tokens.next();
                }
                _ => {}
            }
        }
    }

    // --- Route Parsing ---

    fn parse_route_list(&mut self) -> ParseResult<Vec<RouteConfig>> {
        let mut routes = Vec::new();

        self.skip_newlines_only();
        let mut route_indent = 0;
        if let Some(TokenType::Indent(n)) = self.peek_kind() {
            route_indent = *n;
        }

        loop {
            self.skip_newlines_only();

            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < route_indent {
                    break;
                }
                self.tokens.next();
            }

            if let Some(TokenType::Dash) = self.peek_kind() {
                self.consume(TokenType::Dash)?;
                routes.push(self.parse_single_route(route_indent)?);
            } else {
                break;
            }
        }
        Ok(routes)
    }

    fn parse_single_route(&mut self, min_indent: usize) -> ParseResult<RouteConfig> {
        let mut route = RouteConfig::default();

        loop {
            self.skip_newlines_only();

            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < min_indent {
                    break;
                }
                self.tokens.next();
            }

            if let Some(TokenType::Dash) = self.peek_kind() {
                break;
            }

            let key_str = match self.peek_kind() {
                Some(TokenType::Text(s)) | Some(TokenType::StringLit(s)) => s.clone(),
                _ => break,
            };

            match key_str.as_str() {
                "path" | "root" | "methods" | "autoindex" | "cgi_ext" | "default_file" | "redirection" | "client_max_body_size" => {
                    self.tokens.next();
                    self.consume(TokenType::Colon)?;

                    match key_str.as_str() {
                        "path" => route.path = self.parse_string()?,
                        "root" => route.root = self.parse_string()?,
                        "default_file" => route.default_file = self.parse_string()?,
                        "methods" => {
                             let raw_methods = self.parse_list(|p| p.parse_string())?;
                             route.methods = raw_methods.into_iter().map(|m| m.to_uppercase()).collect();
                        },
                        "autoindex" => {
                            let val = self.parse_string()?;
                            route.autoindex = val == "true" || val == "on";
                        }
                        "cgi_ext" => route.cgi_ext = Some(self.parse_string()?),
                        "redirection" => route.redirection = Some(self.parse_string()?),
                        "client_max_body_size" => route.client_max_body_size = self.parse_number()? as usize,
                        _ => {}
                    }
                }
                _ => break,
            }
        }
        Ok(route)
    }

    // --- Primitive Parsing ---

    fn parse_error_pages(&mut self) -> ParseResult<HashMap<u16, String>> {
        let mut pages = HashMap::new();
        
        self.skip_newlines_only();
        let mut map_indent = 0;
        if let Some(TokenType::Indent(n)) = self.peek_kind() {
            map_indent = *n;
        }

        loop {
            self.skip_newlines_only();

            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < map_indent {
                    break;
                }
                self.tokens.next();
            } else if map_indent > 0 {
                break;
            }

            let error_code = match self.peek_kind() {
                Some(TokenType::Number(n)) => *n as u16,
                _ => break,
            };
            self.tokens.next();

            self.consume(TokenType::Colon)?;
            let path = self.parse_string()?;
            
            pages.insert(error_code, path);
        }
        
        Ok(pages)
    }

    // Generic list parser: Handles both [ a, b ] and - a \n - b styles
    fn parse_list<T, F>(&mut self, item_parser: F) -> ParseResult<Vec<T>>
    where
        F: Fn(&mut Self) -> ParseResult<T>,
    {
        let mut items = Vec::new();
        self.skip_newlines_only();

        if let Some(TokenType::LBracket) = self.peek_kind() {
            // Flow Style [ ... ]
            self.consume(TokenType::LBracket)?;
            loop {
                while matches!(self.peek_kind(), Some(TokenType::Newline) | Some(TokenType::Indent(_))) {
                    self.tokens.next();
                }

                if let Some(TokenType::RBracket) = self.peek_kind() {
                    self.consume(TokenType::RBracket)?;
                    break;
                }

                items.push(item_parser(self)?);

                if let Some(TokenType::Comma) = self.peek_kind() {
                    self.consume(TokenType::Comma)?;
                }
            }
        } else {
            // Block Style
            // - item
            // - item
            let mut list_indent = 0;
            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                list_indent = *n;
            }

            loop {
                self.skip_newlines_only();

                // Check indent
                if let Some(TokenType::Indent(n)) = self.peek_kind() {
                    if *n < list_indent { break; }
                    self.tokens.next(); // Consume valid indent
                } else if list_indent > 0 {
                    // Expect indent but found none -> dedent
                    break;
                }

                if let Some(TokenType::Dash) = self.peek_kind() {
                    self.consume(TokenType::Dash)?;
                    items.push(item_parser(self)?);
                } else {
                    break;
                }
            }
        }
        Ok(items)
    }

    fn parse_string(&mut self) -> ParseResult<String> {
        match self.tokens.next() {
            Some(t) => match t.kind {
                TokenType::Text(s) | TokenType::StringLit(s) => Ok(s),
                _ => Err(format!("Expected string, found {:?} at line {}", t.kind, t.loc.line)),
            },
            None => Err("Unexpected EOF".to_string()),
        }
    }

    fn parse_number(&mut self) -> ParseResult<u64> {
        match self.tokens.next() {
            Some(t) => match t.kind {
                TokenType::Number(n) => Ok(n),
                _ => Err(format!("Expected number, found {:?} at line {}", t.kind, t.loc.line)),
            },
            None => Err("Unexpected EOF".to_string()),
        }
    }
}

// --- Display Logic ---

pub fn display_config(configs: &Vec<ServerConfig>) {
    println!("\n\x1b[1;35m 🌐 SERVER CONFIGURATION DASHBOARD\x1b[0m");
    println!("\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");

    for (i, server) in configs.iter().enumerate() {
        println!("\n  \x1b[1;37mSERVER BLOCK {:02}\x1b[0m", i + 1);
        // Use the Display implementation
        print!("{}", server);
    }
    
    println!("\n\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");
    println!(" \x1b[1;32m✔\x1b[0m Configuration loaded successfully - Ready for requests!\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn test_config_parsing() {
        // Read file (simulated or real if exists)
        let path = "config.yaml";
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path).unwrap();
            let mut lexer = Lexer::new(&content);
            let tokens = lexer.tokenize().expect("Failed to tokenize");
            let mut parser = ConfigParser::new(tokens);
            let configs = parser.parse().expect("Failed to parse config");
            
            // Basic assertions
            assert!(configs.len() >= 1);
            let first = &configs[0];
            assert_eq!(first.host, "127.0.0.1");
            assert!(first.ports.len() > 0);
        }
    }
}
