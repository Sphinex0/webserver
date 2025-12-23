use std::{
    collections::HashMap,
    io::{self, ErrorKind, Read, Write},
};

use mio::{
    Events, Interest, Poll, Token,
    event::Event,
    net::{TcpListener, TcpStream},
};
use server::error::Result;

const LISTENER_TOKEN: Token = Token(0);

const TOKEN_START: usize = 1;

struct HttpConnection {
    stream: TcpStream,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    is_closing: bool,
}
impl HttpConnection {
    fn new(stream: TcpStream) -> HttpConnection {
        HttpConnection {
            stream,
            read_buffer: Vec::with_capacity(4096),
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
            println!("-------------****");
            let mut temp_buf = [0u8; 10];
            match conn.stream.read(&mut temp_buf) {
                Ok(0) => {
                    println!("client disconnected, closing the connection: {:?}", token);
                    conn.is_closing = true;
                }
                Ok(bytes) => {
                    dbg!(&temp_buf[..bytes]);
                    println!("Received {} bytes from {:?}", bytes, token);

                    // conn.write_buffer.extend_from_slice(&conn.read_buffer);
                    conn.write_buffer.extend_from_slice(&temp_buf);
                    conn.read_buffer.clear();
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    println!("would block error");
                }
                Err(e) => {
                    println!("Read error on {:?}: {}", token, e);
                    conn.is_closing = true;
                }
            }
        }

        if event.is_writable(){
            if !conn.write_buffer.is_empty() {
                let bytes_written = conn.stream.write(&conn.write_buffer)?;
                conn.write_buffer.drain(..bytes_written);
            }
        }


        // if conn.is_closing && conn.write_buffer.is_empty(){
        //     self.connections.remove(&token);
        // }

        Ok(())
    }
}

fn main() -> Result<()> {
    Server::new("127.0.0.1:8080")?.run()?;
    Ok(())
}
