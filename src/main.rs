use std::{
    collections::HashMap,
    io::{self, ErrorKind, Read, Write},
    sync::Arc,
};

use mio::{
    Events, Interest, Poll, Token,
    event::Event,
    net::{TcpListener, TcpStream},
};
use server::{
    config::{ConfigParser, ServerConfig},
    httpParser::{HttpRequest, ParsingState},
};

pub struct HttpConnection {
    stream: TcpStream,
    request: HttpRequest,
    write_buffer: Vec<u8>,
    is_closing: bool,
    // Every connection points back to its specific server's rules
    config: Arc<ServerConfig>,
}

impl HttpConnection {
    fn new(stream: TcpStream, config: Arc<ServerConfig>) -> Self {
        Self {
            stream,
            request: HttpRequest::new(),
            write_buffer: Vec::new(),
            is_closing: false,
            config,
        }
    }
}

pub struct Server {
    poll: Poll,
    // Token -> (Listener, Config)
    listeners: HashMap<Token, (TcpListener, Arc<ServerConfig>)>,
    connections: HashMap<Token, HttpConnection>,
    next_token: usize,
}

impl Server {
    pub fn new(configs: Vec<ServerConfig>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let mut listeners = HashMap::new();
        let mut next_token = 1;

        for config in configs {
            let shared_config = Arc::new(config);
            // Iterate through the array of ports for THIS server block
            for port in &shared_config.ports {
                let addr_str = format!("{}:{}", shared_config.host, port);
                let addr = addr_str
                    .parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid address"))?;

                let mut listener = TcpListener::bind(addr)?;
                let token = Token(next_token);

                poll.registry()
                    .register(&mut listener, token, Interest::READABLE)?;

                listeners.insert(token, (listener, Arc::clone(&shared_config)));
                next_token += 1;
                println!("Listening on {}", addr_str);
            }
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
        // Get the specific listener and its config
        let (listener, config) = self.listeners.get(&token).unwrap();
        let config_ref = Arc::clone(config);

        loop {
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    let client_token = Token(self.next_token);
                    self.next_token += 1;

                    self.poll
                        .registry()
                        .register(&mut stream, client_token, Interest::READABLE)?;

                    // Create connection with the correct config for this port
                    let conn = HttpConnection::new(stream, config_ref.clone());
                    self.connections.insert(client_token, conn);
                    println!("Accepted {} on port {:?}", addr, config_ref.ports);
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
            dbg!(1);
            let mut stack_buf = [0u8; 10];
            loop {
                match conn.stream.read(&mut stack_buf) {
                    Ok(0) => {
                        conn.is_closing = true;
                        break;
                    }
                    Ok(n) => {
                        dbg!(&n);
                        conn.request.append_data(&stack_buf[..n]);
                        match conn.request.parse() {
                            // Ok(ParsingState::Headers) => {
                            //     let body_size = conn
                            //         .request
                            //         .headers
                            //         .get("content-length")
                            //         .unwrap()
                            //         .parse::<usize>()
                            //         .unwrap();
                            //     if body_size > conn.config.client_max_body_size {
                            //         Self::queue_error(conn, self.poll.registry(), token, 413);
                            //         break;
                            //     }
                            // }
                            Ok(ParsingState::Complete) => {
                                // NEW: Route the request using the injected config
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
                let bytes_written = conn.stream.write(&conn.write_buffer)?;
                conn.write_buffer.drain(..bytes_written);
                if conn.write_buffer.is_empty() {
                    self.poll
                        .registry()
                        .reregister(&mut conn.stream, token, Interest::READABLE)?;
                }

                if !conn.request.buffer.is_empty() {
                    match conn.request.parse() {
                        Ok(ParsingState::Complete) => {
                            // NEW: Route the request using the injected config
                            Self::process_request(conn, self.poll.registry(), token)?;
                            conn.request.clear();
                        }
                        _ => {}
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

        // Check if there is a custom error page in the config
        let body = if let Some(path) = conn.config.error_pages.get(&status_code) {
            std::fs::read_to_string(path)
                .unwrap_or_else(|_| format!("<h1>{} {}</h1>", status_code, error_msg))
        } else {
            format!("<h1>{} {}</h1>", status_code, error_msg)
        };

        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status_code,
            error_msg,
            body.len(),
            body
        );

        conn.write_buffer.extend_from_slice(response.as_bytes());
        conn.is_closing = true; // Signal to close after writing

        // Register for WRITABLE to ensure the error gets sent
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
        let method = conn.request.methode.clone();

        // 1. Longest Prefix Match to find the route
        let route = match conn.config.find_route(&path) {
            Some(r) => r,
            None => {
                Self::queue_error(conn, registry, token, 404);
                return Ok(());
            }
        };

        // 2. Check Methods
        if !route.methods.contains(&method) {
            Self::queue_error(conn, registry, token, 405);
            return Ok(());
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
                Err(err) => {
                    Self::queue_error(conn, registry, token, 404);
                }
            }
        }
        // 4. POST and DELETE logic would go here

        // Finalize for sending
        registry.reregister(
            &mut conn.stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        )?;

        Ok(())
    }
}

fn main() -> io::Result<()> {
    // 1. Read and Parse Config
    let config_path = "config.yaml";
    let raw_config = std::fs::read_to_string(config_path).expect("Failed to read config file");

    // Assume ConfigParser returns Vec<ServerConfig>
    let mut parser = ConfigParser::new(raw_config);
    let configs = parser.parse();

    // 2. Initialize Server with all configs
    let mut server = Server::new(configs)?;

    // 3. Run the event loop
    server.run()
}
