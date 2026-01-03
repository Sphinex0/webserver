use std::{
    collections::HashMap,
    io::{self, ErrorKind, Read, Write},
    sync::Arc,
};

use mio::{
    event::Event,
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};

use crate::{
    config::ServerConfig,
    http_parser::{HttpRequest, ParsingState},
};

pub struct HttpConnection {
    stream: TcpStream,
    request: HttpRequest,
    write_buffer: Vec<u8>,
    is_closing: bool,
    // Possible configurations for this connection (same Host:Port, different server_name)
    candidates: Vec<Arc<ServerConfig>>,
}

impl HttpConnection {
    fn new(stream: TcpStream, candidates: Vec<Arc<ServerConfig>>) -> Self {
        Self {
            stream,
            request: HttpRequest::new(),
            write_buffer: Vec::new(),
            is_closing: false,
            candidates,
        }
    }

    /// Resolves the correct configuration based on the Host header.
    /// Defaults to the first candidate (or the one marked default_server) if no match.
    fn resolve_config(&self) -> Arc<ServerConfig> {
        if let Some(host_header) = self.request.headers.get("Host") {
            // Host header might be "example.com:8080", we usually just care about the name "example.com"
            // but strict matching might require checking the port too. 
            // For now, let's split off the port if present.
            let hostname = host_header.split(':').next().unwrap_or("");

            for config in &self.candidates {
                if config.server_name == hostname {
                    return Arc::clone(config);
                }
            }
        }

        // If no match found, find the default_server, or fallback to the first one
        for config in &self.candidates {
            if config.default_server {
                return Arc::clone(config);
            }
        }

        // Fallback to the first one
        Arc::clone(&self.candidates[0])
    }
}

pub struct Server {
    poll: Poll,
    // Token -> (Listener, List of Configs for this port)
    listeners: HashMap<Token, (TcpListener, Vec<Arc<ServerConfig>>)>, 
    connections: HashMap<Token, HttpConnection>,
    next_token: usize,
}

impl Server {
    pub fn new(configs: Vec<ServerConfig>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let mut listeners = HashMap::new();
        let mut next_token = 1;

        // Group configs by (Host, Port)
        let mut groups: HashMap<(String, u16), Vec<Arc<ServerConfig>>> = HashMap::new();

        for config in configs {
            let shared_config = Arc::new(config);
            for port in &shared_config.ports {
                let key = (shared_config.host.clone(), *port);
                groups.entry(key).or_default().push(Arc::clone(&shared_config));
            }
        }

        for ((host, port), config_list) in groups {
            let addr_str = format!("{}:{}", host, port);
            let addr = addr_str
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid address: {}", addr_str)))?;

            let mut listener = TcpListener::bind(addr)?;
            let token = Token(next_token);

            poll.registry()
                .register(&mut listener, token, Interest::READABLE)?;

            listeners.insert(token, (listener, config_list));
            next_token += 1;
            println!("Listening on {}", addr_str);
        }

        Ok(Server {
            poll,
            listeners,
            connections: HashMap::new(),
            next_token: 1000,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut events = Events::with_capacity(1024);
        loop {
            self.poll.poll(&mut events, None)?;
            for event in events.iter() {
                let token = event.token();

                // 1. Check if token belongs to one of our listeners
                if self.listeners.contains_key(&token) {
                    self.handle_listener_event(token)?;
                } else {
                    // 2. Otherwise handle as a client connection
                    self.handle_connection_event(token, event)?;
                }
            }
        }
    }

    fn handle_listener_event(&mut self, token: Token) -> io::Result<()> {
        // Get the specific listener and its candidates
        let (listener, candidates) = self.listeners.get(&token).unwrap();
        // We can't clone candidates inside the match because we borrow listener.
        // But we can clone the Arc list before looping or inside the loop if we are careful.
        // Actually, Vec<Arc<...>> is cheap to clone.
        let candidates_clone = candidates.clone();

        loop {
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    let client_token = Token(self.next_token);
                    self.next_token += 1;

                    self.poll
                        .registry()
                        .register(&mut stream, client_token, Interest::READABLE)?;

                    // Create connection with the candidates
                    let conn = HttpConnection::new(stream, candidates_clone.clone());
                    self.connections.insert(client_token, conn);
                    println!("Accepted {} on listener token {:?}", addr, token);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn handle_connection_event(&mut self, token: Token, event: &Event) -> io::Result<()> {
        let conn = match self.connections.get_mut(&token) {
            Some(c) => c,
            None => return Ok(()),
        };

        if event.is_readable() {
            let mut stack_buf = [0u8; 1024]; // Increased buffer size for efficiency
            loop {
                match conn.stream.read(&mut stack_buf) {
                    Ok(0) => {
                        conn.is_closing = true;
                        break;
                    }
                    Ok(n) => {
                        conn.request.append_data(&stack_buf[..n]);
                        match conn.request.parse() {
                            Ok(ParsingState::Complete) => {
                                Self::process_request(conn, self.poll.registry(), token)?;
                                conn.request.clear();
                                break;
                            }
                            _ => {}
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => {
                        conn.is_closing = true;
                        break;
                    }
                }
            }
        }
        
        if event.is_writable() {
            if !conn.write_buffer.is_empty() {
                // We need to handle potential partial writes
                match conn.stream.write(&conn.write_buffer) {
                    Ok(bytes_written) => {
                        conn.write_buffer.drain(..bytes_written);
                        if conn.write_buffer.is_empty() {
                            self.poll
                                .registry()
                                .reregister(&mut conn.stream, token, Interest::READABLE)?;
                        }
                         // Check pipeline processing
                         if conn.write_buffer.is_empty() && !conn.request.buffer.is_empty() {
                            match conn.request.parse() {
                                Ok(ParsingState::Complete) => {
                                    Self::process_request(conn, self.poll.registry(), token)?;
                                    conn.request.clear();
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {},
                    Err(_) => {
                        conn.is_closing = true;
                    }
                }
            }
        }

        if conn.is_closing && conn.write_buffer.is_empty() {
            self.connections.remove(&token);
        }

        Ok(())
    }

    fn queue_error(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        status_code: u16,
    ) {
        let error_msg = match status_code {
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            _ => "Internal Server Error",
        };

        // Resolve config to check for custom error pages
        let config = conn.resolve_config();

        let body = if let Some(path) = config.error_pages.get(&status_code) {
            std::fs::read_to_string(path)
                .unwrap_or_else(|_| format!("<h1>{} {}</h1>", status_code, error_msg))
        } else {
            format!("<h1>{} {}</h1>", status_code, error_msg)
        };

        let response = format!(
            "HTTP/1.1 {} {}
Content-Length: {}
Connection: close

{}",
            status_code,
            error_msg,
            body.len(),
            body
        );

        conn.write_buffer.extend_from_slice(response.as_bytes());
        conn.is_closing = true;

        let _ = registry.reregister(
            &mut conn.stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        );
    }

    fn process_request(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
    ) -> io::Result<()> {
        let path = conn.request.path.clone();
        let method = conn.request.method.clone();

        // Resolve the correct configuration for this request
        let config = conn.resolve_config();

        // 1. Longest Prefix Match to find the route
        let route = match config.find_route(&path) {
            Some(r) => r,
            None => {
                Self::queue_error(conn, registry, token, 404);
                return Ok(())
            }
        };

        // 2. Check Methods
        if !route.methods.contains(&method) {
            Self::queue_error(conn, registry, token, 405);
            return Ok(())
        }

        // 3. Simple GET implementation (Static Files)
        if method == "GET" {
            let full_path;
            if !route.default_file.is_empty() && path == "/" {
                full_path = format!("{}/{}", route.root, route.default_file);
            } else {
                full_path = format!("{}{}", route.root, path);
            }
            match std::fs::read(&full_path) {
                Ok(content) => {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        content.len()
                    );
                    conn.write_buffer.extend_from_slice(response.as_bytes());
                    conn.write_buffer.extend_from_slice(&content);
                }
                Err(_err) => {
                    Self::queue_error(conn, registry, token, 404);
                }
            }
        }
        // 4. POST and DELETE logic would go here

        registry.reregister(
            &mut conn.stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        )?;

        Ok(())
    }
}