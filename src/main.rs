use std::{
    collections::HashMap, io, net::{TcpListener, TcpStream}, task::Poll
};

use mio::{Poll, Token};
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
        let listener = TcpListener::bind(addr.parse().unwrap())?;
        let poll = Poll::new();

        Ok(Server { listener, poll, connections: HashMap::new(), next_token: TOKEN_START })

    }
}

fn main() -> Result<()> {
    Ok(())
}
