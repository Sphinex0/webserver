use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RouteConfig {
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
    lines: std::iter::Peekable<std::vec::IntoIter<String>>,
}

impl ConfigParser {
    pub fn new(content: String) -> Self {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        Self {
            lines: lines.into_iter().peekable(),
        }
    }

    pub fn parse(&mut self) -> Vec<ServerConfig> {
        let mut servers = Vec::new();
        while let Some(line) = self.lines.next() {
            let trimmed = line.trim();
            if trimmed == "server:" {
                servers.push(self.parse_server());
            }
        }
        servers
    }

    fn parse_server(&mut self) -> ServerConfig {
        let mut config = ServerConfig {
            host: String::from("127.0.0.1"),
            ports: Vec::new(),
            server_name: String::new(),
            default_server: false,
            error_pages: HashMap::new(),
            client_max_body_size: 1024 * 1024, // 1MB
            routes: HashMap::new(),
        };

        while let Some(line) = self.lines.peek() {
            let indent = line.len() - line.trim_start().len();
            if indent == 0 && !line.trim().is_empty() {
                break;
            } // End of server block

            let line = self.lines.next().unwrap();
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() < 2 {
                continue;
            }

            let key = parts[0].trim();
            let val = parts[1].trim();

            match key {
                "listen" => config.ports.push(val.parse().unwrap()),
                "host" => config.host = val.to_string(),
                "server_name" => config.server_name = val.to_string(),
                "error_page" => {
                    let err_parts: Vec<&str> = val.split_whitespace().collect();
                    if err_parts.len() == 2 {
                        config
                            .error_pages
                            .insert(err_parts[0].parse().unwrap(), err_parts[1].to_string());
                    }
                }
                "location" => {
                    let path = val.to_string();
                    config.routes.insert(path, self.parse_route());
                }
                _ => {}
            }
        }
        config
    }

    fn parse_route(&mut self) -> RouteConfig {
        let mut route = RouteConfig {
            methods: Vec::new(),
            redirection: None,
            root: String::from("./html"),
            default_file: String::from("index.html"),
            cgi_ext: None,
            autoindex: false,
            client_max_body_size: 1024 * 1024,
        };
        // Similar logic to parse_server, but for route-specific keys (methods, root, etc.)
        // We look for deeper indentation here.

        while let Some(line) = self.lines.peek() {
            let _indent = line.len() - line.trim_start().len();
            if line.trim().is_empty() {
                break;
            } // End of location block

            let line = self.lines.next().unwrap();
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() < 2 {
                continue;
            }

            let key = parts[0].trim();
            let val = parts[1].trim();

            match key {
                "methods" => route
                    .methods
                    .extend(val.trim().split(" ").map(|s| s.to_owned())),
                "root" => route.root = val.to_string(),
                "default_file" => route.default_file = val.to_string(),
                "autoindex" => route.autoindex = val.parse().unwrap(),
                "cgi_ext" => route.cgi_ext = Some(val.to_string()),
                "redirection" => route.redirection = Some(val.to_string()),
                _ => {}
            }
        }
        route
    }
}

#[test]
fn test_config_parsing() {
    let content = std::fs::read_to_string("config.yaml").unwrap();
    let mut parser = ConfigParser::new(content);
    let configs = parser.parse();
    // println!("{configs:#?}");
    assert_eq!(configs.len(), 2);
    assert!(configs[0].ports.contains(&8080));
    assert!(configs[0].ports.contains(&9000));
    assert_eq!(configs[1].server_name, "secondary.com");
}



pub fn display_config(configs: &Vec<ServerConfig>) {
    // Clear screen (optional, but professional)
    // print!("\x1b[2J\x1b[1;1H");

    println!("\n\x1b[1;35m 🌐 01_server CONFIGURATION DASHBOARD\x1b[0m");
    println!("\x1b[38;5;240m ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");

    for (i, server) in configs.iter().enumerate() {
        let server_label = format!("SERVER BLOCK {:02}", i + 1);
        println!("\n  \x1b[1;37m{}\x1b[0m", server_label);
        println!("  \x1b[38;5;244m─────────────────────────────────────────\x1b[0m");

        // Info Grid
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mNetwork:\x1b[0m    \x1b[32m{}\x1b[0m \x1b[38;5;244mvia ports\x1b[0m \x1b[1;32m{:?}\x1b[0m",
            server.host, server.ports
        );
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mIdentitie:\x1b[0m  \x1b[36m{}\x1b[0m",
            server.server_name
        );
        println!(
            "  \x1b[1;34m⦿\x1b[0m \x1b[1;37mLimits:\x1b[0m     \x1b[33m{} bytes\x1b[0m \x1b[38;5;244m(Max Body)\x1b[0m",
            server.client_max_body_size
        );

        println!("\n  \x1b[1;37mRouting Table:\x1b[0m");

        // Collect routes and sort them for a stable display
        let mut sorted_routes: Vec<_> = server.routes.iter().collect();
        sorted_routes.sort_by(|a, b| a.0.cmp(b.0));

        for (idx, (path, route)) in sorted_routes.iter().enumerate() {
            let is_last = idx == sorted_routes.len() - 1;
            let branch = if is_last {
                "  └──"
            } else {
                "  ├──"
            };
            let methods_fmt = route.methods.join("|");

            // Using ANSI background for methods makes them pop
            println!(
                "  \x1b[38;5;244m{}\x1b[0m \x1b[1;37m{:12}\x1b[0m \x1b[48;5;236m\x1b[38;5;250m {} \x1b[0m ➔ \x1b[38;5;244mroot:\x1b[0m \x1b[3m{}\x1b[0m",
                branch, path, methods_fmt, route.root
            );

            if let Some(cgi) = &route.cgi_ext {
                let cgi_branch = if is_last { "     " } else { "  │  " };
                println!(
                    "  \x1b[38;5;244m{}  └─ \x1b[0m\x1b[38;5;208mCGI Enabled: {}\x1b[0m",
                    cgi_branch, cgi
                );
            }
        }
    }
    println!(
        "\n\x1b[38;5;240m ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m"
    );
    println!(" \x1b[1;32m✔\x1b[0m Server initialized and ready for events.\n");
}
