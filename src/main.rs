use std::{
    collections::HashMap,
    io::{self, ErrorKind, Read, Write},
};

use mio::{
    Events, Interest, Poll, Token,
    event::Event,
    net::{TcpListener, TcpStream},
};
use server::{
    error::Result,
    httpParser::{HttpRequest, ParsingState}, router::Router,
};

const LISTENER_TOKEN: Token = Token(0);

const TOKEN_START: usize = 1;

struct HttpConnection {
    stream: TcpStream,
    // read_buffer: Vec<u8>,
    request: HttpRequest,
    write_buffer: Vec<u8>,
    is_closing: bool,
}
impl HttpConnection {
    fn new(stream: TcpStream) -> HttpConnection {
        HttpConnection {
            stream,
            // read_buffer: Vec::with_capacity(4096),
            request: HttpRequest::new(),
            write_buffer: Vec::new(),
            is_closing: false,
        }
    }
}

pub struct Server {
    listener: TcpListener,
    poll: Poll,
    connections: HashMap<Token, HttpConnection>,
    next_token: usize,
    router: Router,
}

impl Server {
    pub fn new(addr: &str) -> io::Result<Server> {
        let mut listener = TcpListener::bind(addr.parse().unwrap())?;
        let poll = Poll::new()?;

        poll.registry()
            .register(&mut listener, LISTENER_TOKEN, Interest::READABLE)?;

        Ok(Server {
            listener,
            poll,
            connections: HashMap::new(),
            next_token: TOKEN_START,
            router: Router::new(),
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut events = Events::with_capacity(1024);

        println!("Server starting...");

        loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                match event.token() {
                    LISTENER_TOKEN => self.handle_listener_event()?,
                    token => self.handle_connection_event(token, event)?,
                }
            }
        }
    }

    // event handlers
    fn handle_listener_event(&mut self) -> io::Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    println!("Accepted new connection from: {}", addr);
                    let token = Token(self.next_token);
                    self.next_token += 1;

                    self.poll.registry().register(
                        &mut stream,
                        token,
                        Interest::READABLE.add(Interest::WRITABLE),
                    )?;

                    let conn = HttpConnection::new(stream);
                    self.connections.insert(token, conn);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    fn handle_connection_event(&mut self, token: Token, event: &Event) -> io::Result<()> {
        let conn = self.connections.get_mut(&token).unwrap();

        if event.is_readable() {
            loop {
                let mut stack_buf = [0u8; 4096];
                match conn.stream.read(&mut stack_buf) {
                    Ok(0) => {
                        println!("client disconnected, closing the connection: {:?}", token);
                        conn.is_closing = true;
                        break;
                    }
                    Ok(bytes) => {
                        println!("Received {} bytes from {:?}", bytes, token);
                        // Attempt to parse what we have so far
                        conn.request.buffer.extend_from_slice(&stack_buf[..bytes]);
                        match conn.request.parse() {
                            Ok(ParsingState::Complete) => {
                                println!("Request fully parsed! Generating response...");
                                let response = self.router.handle(&conn.request);
                                conn.write_buffer.extend_from_slice(response.as_bytes());
                                // conn.write_buffer.extend_from_slice(
                                //     b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, World!",
                                // );
                                // conn.request.buffer.clear();
                                self.poll.registry().reregister(
                                    &mut conn.stream,
                                    token,
                                    Interest::READABLE.add(Interest::WRITABLE),
                                )?;
                            }
                            Ok(_) => {} // Still parsing (RequestLine or Headers)
                            Err(e) => {
                                eprintln!("Parsing error: {}", e);
                                conn.is_closing = true;
                                break;
                            }
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                        println!("would block error");
                        break;
                    }
                    Err(e) => {
                        println!("Read error on {:?}: {}", token, e);
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
            }
        }

        if conn.is_closing && conn.write_buffer.is_empty() {
            self.connections.remove(&token);
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let mut server = Server::new("127.0.0.1:8080")?;

    server.router.add_route("/welcome", welcome);
    server.run()?;
    Ok(())
}


pub fn welcome(request: &HttpRequest)-> String{
    return "HTTP/1.1 200 OK\r\nContent-Length: 26\r\n\r\nHello, how are you doing ?".to_string();
}