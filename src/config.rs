use std::{collections::HashMap, iter::Peekable};

use crate::lexer::tokens::{Loc, Token, TokenType};

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

// Struct definitions for compilation context
// #[derive(Debug)]
// pub struct ServerConfig {
//     pub host: String,
//     pub ports: Vec<u16>,
//     pub routes: HashMap<String, RouteConfig>,
// }
// #[derive(Debug)]
// pub struct RouteConfig {
//     pub path: String,
//     pub methods: Vec<String>,
//     pub root: String,
// }

impl ServerConfig {
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

pub struct ConfigParser {
    tokens: Peekable<std::vec::IntoIter<Token>>,
}

impl ConfigParser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
        }
    }

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

    // --- Entry Point ---
    pub fn parse(&mut self) -> Result<Vec<ServerConfig>, String> {
        // Skip initial newlines/indents
        self.skip_newlines();

        // Expect "servers:"
        match self.tokens.next() {
            Some(t) if matches!(t.kind, TokenType::Text(ref s) if s == "servers") => {}
            _ => return Err("Config must start with 'servers:'".to_string()),
        }
        self.consume(TokenType::Colon)?;

        // Parse the list of servers (Block style usually)
        let mut servers = Vec::new();
        self.skip_newlines();

        while let Some(TokenType::Dash) = self.peek_kind() {
            self.consume(TokenType::Dash)?; // Consume '-'
            servers.push(self.parse_server_block()?);
            self.skip_newlines();
        }
        // dbg!(&servers);

        Ok(servers)
    }

    // --- Server Block Parsing ---
    fn parse_server_block(&mut self) -> Result<ServerConfig, String> {
        let mut config = ServerConfig {
            host: "127.0.0.1".to_string(),
            ports: vec![8080],
            server_name: "_".to_string(), // Common convention for a "catch-all" or default name
            default_server: false,
            error_pages: HashMap::new(),
            client_max_body_size: 1_048_576, // 1MB in bytes (1024 * 1024)
            routes: HashMap::new(),
        };

        // Loop until we see a Token that implies end of block (like a new Dash or EOF)
        loop {
            self.skip_newlines();
            // Check if we are done with this server (next is '-' for new server or EOF)
            match self.peek_kind() {
                Some(TokenType::Dash) | None => break,
                Some(TokenType::Indent(_)) => {
                    self.tokens.next();
                    continue;
                } // Consume indents inside block
                _ => {}
            }

            // Parse Key (Text or StringLit)
            let key = match self.tokens.next() {
                Some(t) => match t.kind {
                    TokenType::Text(s) | TokenType::StringLit(s) => s,
                    _ => {
                        return Err(format!(
                            "Expected Key, found {:?} at line {}",
                            t.kind, t.loc.line
                        ));
                    }
                },
                None => break,
            };

            self.consume(TokenType::Colon)?; // Lexer handles the space before this!

            match key.as_str() {
                "host" => config.host = self.parse_string()?,
                "ports" => config.ports = self.parse_u16_list()?,
                "routes" => {
                    let routes_list = self.parse_route_list()?;
                    for route in routes_list {
                        // Use the path as the key for the HashMap
                        config.routes.insert(route.path.clone(), route);
                    }
                }
                "server_name" => config.server_name = self.parse_string()?,
                "error_pages" => config.error_pages = self.parse_error_pages()?,
                _ => {
                    // Consume unknown scalar value to prevent crash
                    if let Some(t) = self.tokens.peek() {
                        match t.kind {
                            TokenType::Text(_) | TokenType::Number(_) | TokenType::StringLit(_) => {
                                self.tokens.next();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(config)
    }

    // --- NEW: Parse Error Pages Map ---
    fn parse_error_pages(&mut self) -> Result<HashMap<u16, String>, String> {
        let mut pages = HashMap::new();
        
        // 1. Establish Baseline Indentation
        self.skip_newlines_only();
        let mut map_indent = 0;
        if let Some(TokenType::Indent(n)) = self.peek_kind() {
            map_indent = *n;
        }

        loop {
            self.skip_newlines_only();

            // 2. Check Indentation Depth
            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < map_indent {
                    // Indent dropped (back to server level). STOP.
                    break;
                }
                self.tokens.next(); // Consume valid indent
            } else if map_indent > 0 {
                // If we expect indent but found none (e.g. next line is "host:"), break.
                break;
            }

            // 3. Parse Key (Error Code)
            // We expect a Number (e.g., 404)
            let error_code = match self.peek_kind() {
                Some(TokenType::Number(n)) => *n as u16,
                _ => break, // Not a number? Stop.
            };
            self.tokens.next(); // Consume the number

            // 4. Consume Separator
            self.consume(TokenType::Colon)?;

            // 5. Parse Value (File Path)
            let path = self.parse_string()?;
            
            pages.insert(error_code, path);
        }
        
        Ok(pages)
    }

    fn parse_route_list(&mut self) -> Result<Vec<RouteConfig>, String> {
        let mut routes = Vec::new();

        // 1. Determine baseline indentation from the first item
        self.skip_newlines_only();
        let mut route_indent = 0;
        if let Some(TokenType::Indent(n)) = self.peek_kind() {
            route_indent = *n;
        }

        loop {
            self.skip_newlines_only();

            // 2. Check Indentation vs Baseline
            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < route_indent {
                    // Dedent detected! (e.g. back to server level). Stop parsing routes.
                    break;
                }
                // If indent matches, it's safe to consume (it belongs to this list)
                self.tokens.next();
            }

            // 3. Check for Dash
            if let Some(TokenType::Dash) = self.peek_kind() {
                self.consume(TokenType::Dash)?;
                // Pass the indentation level to the single route parser!
                routes.push(self.parse_single_route(route_indent)?);
            } else {
                break;
            }
        }
        Ok(routes)
    }

    // Now accepts min_indent
    fn parse_single_route(&mut self, min_indent: usize) -> Result<RouteConfig, String> {
        let mut route = RouteConfig {
            path: "/".to_string(),
            methods: vec!["GET".to_string(), "HEAD".to_string()],
            root: "./www".to_string(),
            autoindex: false,
            // ... other defaults
            cgi_ext: None,
            default_file: "index.html".to_string(),
            redirection: None,
            client_max_body_size: 1000000,
        };

        loop {
            self.skip_newlines_only();

            // --- THE FIX: Smart Indent Check ---
            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                if *n < min_indent {
                    // This indent belongs to the PARENT (Server block).
                    // DO NOT CONSUME. BREAK IMMEDIATELY.
                    break;
                }

                // If indent is exactly min_indent, it might be the next Dash (Next Route)
                // We let the logic below checking for Dash handle that break.
                // We consume the indent here so we can see the Key or Dash.
                self.tokens.next();
            }
            // -----------------------------------

            // Check for next dash (Next route item)
            if let Some(TokenType::Dash) = self.peek_kind() {
                break;
            }

            // PEEK Key
            let key_str = match self.peek_kind() {
                Some(TokenType::Text(s)) | Some(TokenType::StringLit(s)) => s.clone(),
                _ => break,
            };

            // CHECK if this key belongs to a Route
            match key_str.as_str() {
                "path" | "root" | "methods" | "autoindex" | "cgi_ext" | "default_file" | "redirection" => {
                    self.tokens.next(); // Consume key
                    self.consume(TokenType::Colon)?;

                    match key_str.as_str() {
                        "path" => route.path = self.parse_string()?,
                        "root" => route.root = self.parse_string()?,
                        "default_file" => route.default_file = self.parse_string()?,
                        "methods" => route.methods = self.parse_string_list()?,
                        "autoindex" => {
                            let val = self.parse_string()?;
                            route.autoindex = val == "true" || val == "on";
                        }
                        "cgi_ext" => route.cgi_ext = Some(self.parse_string()?),
                        "redirection" => route.redirection = Some(self.parse_string()?),

                        // Handle other fields...
                        _ => {}
                    }
                }
                // If it's "host", "ports", etc. -> BREAK (It belongs to the Server)
                _ => break,
            }
        }
        Ok(route)
    }

    fn skip_newlines_only(&mut self) {
        while let Some(TokenType::Newline) = self.peek_kind() {
            self.tokens.next();
        }
    }

    // --- List Parsing Helpers ---

    fn parse_u16_list(&mut self) -> Result<Vec<u16>, String> {
        let mut nums = Vec::new();
        self.skip_newlines_only(); // Stop at Indent or LBracket/Dash

        if let Some(TokenType::LBracket) = self.peek_kind() {
            // --- Flow Style [ ... ] --- (This part was already working)
            self.consume(TokenType::LBracket)?;
            loop {
                // Ignore formatting inside brackets
                while matches!(
                    self.peek_kind(),
                    Some(TokenType::Newline) | Some(TokenType::Indent(_))
                ) {
                    self.tokens.next();
                }

                match self.peek_kind() {
                    Some(TokenType::Number(_)) => {
                        if let Some(TokenType::Number(n)) = self.tokens.next().map(|t| t.kind) {
                            nums.push(n as u16);
                        }
                    }
                    Some(TokenType::RBracket) => {
                        self.consume(TokenType::RBracket)?;
                        break;
                    }
                    Some(TokenType::Comma) => {
                        self.consume(TokenType::Comma)?;
                    }
                    _ => return Err("Invalid token in ports list".to_string()),
                }
            }
        } else {
            // --- Block Style "- 80" --- (FIXED)

            // 1. Establish Baseline Indentation
            let mut list_indent = 0;
            if let Some(TokenType::Indent(n)) = self.peek_kind() {
                list_indent = *n;
                // Note: We don't consume the indent here yet, we let the loop handle it
                // OR we assume the first item defines the indent.
            }

            loop {
                self.skip_newlines_only();

                // 2. Check Indentation Depth
                if let Some(TokenType::Indent(n)) = self.peek_kind() {
                    if *n < list_indent {
                        // Indent dropped (back to server level). STOP.
                        break;
                    }
                    self.tokens.next(); // Consume valid indent
                } else if list_indent > 0 {
                    // If we expect indent but found none (and not EOF), we might be at a dedent (0 indent)
                    // If the previous line had indent 4, and this one has 0, we break.
                    break;
                }

                // 3. Check for Dash
                if let Some(TokenType::Dash) = self.peek_kind() {
                    self.consume(TokenType::Dash)?;
                    if let Some(TokenType::Number(n)) = self.tokens.next().map(|t| t.kind) {
                        nums.push(n as u16);
                    } else {
                        return Err("Expected port number after dash".to_string());
                    }
                } else {
                    // No dash? Stop.
                    break;
                }
            }
        }
        Ok(nums)
    }

    // Handles ["GET", POST]
    fn parse_string_list(&mut self) -> Result<Vec<String>, String> {
        let mut strs = Vec::new();
        self.skip_newlines();

        if let Some(TokenType::LBracket) = self.peek_kind() {
            self.consume(TokenType::LBracket)?;
            loop {
                // Skip filler inside brackets
                while matches!(
                    self.peek_kind(),
                    Some(TokenType::Newline) | Some(TokenType::Indent(_))
                ) {
                    self.tokens.next();
                }

                match self.peek_kind() {
                    Some(TokenType::Text(_)) | Some(TokenType::StringLit(_)) => {
                        strs.push(self.parse_string()?);
                    }
                    Some(TokenType::RBracket) => {
                        self.consume(TokenType::RBracket)?;
                        break;
                    }
                    Some(TokenType::Comma) => {
                        self.consume(TokenType::Comma)?;
                    }
                    _ => return Err("Invalid token in string list".to_string()),
                }
            }
        }
        Ok(strs)
    }

    fn parse_string(&mut self) -> Result<String, String> {
        match self.tokens.next() {
            Some(t) => match t.kind {
                TokenType::Text(s) | TokenType::StringLit(s) => Ok(s),
                _ => Err(format!("Expected string, found {:?}", t.kind)),
            },
            None => Err("Unexpected EOF".to_string()),
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
}

#[test]
fn test_config_parsing() {
    let content = std::fs::read_to_string("config.yaml").unwrap();
    // let mut parser = ConfigParser::new(content);
    // let configs = parser.parse();
    // println!("{configs:#?}");
    // assert_eq!(configs.len(), 2);
    // assert!(configs[0].ports.contains(&8080));
    // assert!(configs[0].ports.contains(&9000));
    // assert_eq!(configs[1].server_name, "secondary.com");
}

pub fn display_config(configs: &Vec<ServerConfig>) {
    println!("\n\x1b[1;35m 🌐 SERVER CONFIGURATION DASHBOARD\x1b[0m");
    println!("\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");

    for (i, server) in configs.iter().enumerate() {
        let server_label = format!("SERVER BLOCK {:02}", i + 1);
        println!("\n  \x1b[1;37m{}\x1b[0m", server_label);
        println!("  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m");

        // Server Info Grid
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mNetwork:\x1b[0m     \x1b[32m{}\x1b[0m \x1b[38;5;244mvia ports\x1b[0m \x1b[1;32m{:?}\x1b[0m",
            server.host, server.ports
        );
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mIdentity:\x1b[0m    \x1b[36m{}\x1b[0m",
            server.server_name
        );
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mDefault:\x1b[0m     \x1b[{}m{}\x1b[0m",
            if server.default_server { "32" } else { "31" }, 
            if server.default_server { "YES" } else { "NO" }
        );
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mBody Limit:\x1b[0m  \x1b[33m{} KB\x1b[0m",
            server.client_max_body_size / 1024
        );

        // Error Pages
        if !server.error_pages.is_empty() {
            println!("  \x1b[1;34m⦿\x1b[0m \x1b[1;37mError Pages:\x1b[0m");
            for (code, path) in &server.error_pages {
                println!("    \x1b[38;5;244m{:4}\x1b[0m → \x1b[31m{}\x1b[0m", code, path);
            }
        }

        // Routes Section
        println!("\n  \x1b[1;37m📋 ROUTING TABLE ({}) \x1b[0m", server.routes.len());
        println!("  \x1b[38;5;244m───────────────────────────────────────────────\x1b[0m");

        let mut sorted_routes: Vec<_> = server.routes.iter().collect();
        sorted_routes.sort_by(|a, b| a.0.cmp(b.0));

        for (idx, (path, route)) in sorted_routes.iter().enumerate() {
            let is_last = idx == sorted_routes.len() - 1;
            let branch = if is_last { "  └──" } else { "  ├──" };
            let methods_fmt = route.methods.join(" | ");
            let route_limit = format!("{} KB", route.client_max_body_size / 1024);

            println!(
                "  \x1b[38;5;244m{}\x1b[0m \x1b[1;37m{}\x1b[0m",
                branch, path
            );
            println!(
                "  \x1b[38;5;250m    ├─ Methods:\x1b[0m \x1b[48;5;236m\x1b[38;5;250m {} \x1b[0m",
                methods_fmt
            );
            println!(
                "  \x1b[38;5;250m    ├─ Root:\x1b[0m    \x1b[32m{}\x1b[0m",
                route.root
            );
            println!(
                "  \x1b[38;5;250m    ├─ Default:\x1b[0m  \x1b[36m{}\x1b[0m",
                route.default_file
            );
            println!(
                "  \x1b[38;5;250m    ├─ Body Limit:\x1b[0m \x1b[33m{}\x1b[0m",
                route_limit
            );
            println!(
                "  \x1b[38;5;250m    ├─ Autoindex:\x1b[0m \x1b[{}m{}\x1b[0m",
                if route.autoindex { "32" } else { "31" },
                if route.autoindex { "ON" } else { "OFF" }
            );

            if let Some(redir) = &route.redirection {
                let indent = if is_last { "     " } else { "  │  " };
                println!("  \x1b[38;5;250m{}├─ Redirect:\x1b[0m \x1b[35m{}\x1b[0m", indent, redir);
            }

            if let Some(cgi) = &route.cgi_ext {
                let indent = if is_last { "     " } else { "  │  " };
                println!("  \x1b[38;5;250m{}└─ CGI:\x1b[0m     \x1b[38;5;208m{}\x1b[0m", indent, cgi);
            } else {
                let indent = if is_last { "     " } else { "  │  " };
                println!("  \x1b[38;5;250m{}└─ CGI:\x1b[0m      \x1b[31mDISABLED\x1b[0m", indent);
            }
            
            if !is_last {
                println!("  \x1b[38;5;244m    │\x1b[0m");
            }
        }
    }
    
    println!("\n\x1b[38;5;240m ════════════════════════════════════════════════════════════════\x1b[0m");
    println!(" \x1b[1;32m✔\x1b[0m Configuration loaded successfully - Ready for requests!\n");
}
