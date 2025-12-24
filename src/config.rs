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
    pub server_names: Vec<String>,
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
        Self { lines: lines.into_iter().peekable() }
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
            server_names: Vec::new(),
            default_server: false,
            error_pages: HashMap::new(),
            client_max_body_size: 1024 * 1024, // 1MB
            routes: HashMap::new(),
        };

        while let Some(line) = self.lines.peek() {
            let indent = line.len() - line.trim_start().len();
            if indent == 0 && !line.trim().is_empty() { break; } // End of server block
            
            let line = self.lines.next().unwrap();
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() < 2 { continue; }
            
            let key = parts[0].trim();
            let val = parts[1].trim();

            match key {
                "listen" => config.ports.push(val.parse().unwrap()),
                "host" => config.host = val.to_string(),
                "server_name" => config.server_names = val.split_whitespace().map(|s| s.to_string()).collect(),
                "error_page" => {
                    let err_parts: Vec<&str> = val.split_whitespace().collect();
                    if err_parts.len() == 2 {
                        config.error_pages.insert(err_parts[0].parse().unwrap(), err_parts[1].to_string());
                    }
                },
                "location" => {
                    let path = val.to_string();
                    config.routes.insert(path, self.parse_route());
                },
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
    assert_eq!(configs[1].server_names[0], "secondary.com");
}