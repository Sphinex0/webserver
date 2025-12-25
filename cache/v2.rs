use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

use server::error::*;

const ADDRESS: &str = "127.0.0.1:8080";
const BUFFER_SIZE: usize = 512;

fn main() -> Result<()> {
    let listener = TcpListener::bind(ADDRESS)?;
    listener.set_nonblocking(true)?;

    println!("Server listening on http://{}", ADDRESS);

    let mut clients: Vec<TcpStream> = Vec::new();
    let mut buffer = [0u8; BUFFER_SIZE];

    loop {
        // Accept new clients (nonblocking)
        match listener.accept() {
            Ok((mut stream, addr)) => {
                stream.set_nonblocking(true)?; // IMPORTANT!
                println!("New client: {}", addr);
                clients.push(stream);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No new connections right now
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }

        // Poll clients
        let mut i = 0;
        while i < clients.len() {
            let mut remove_client = false;

            match clients[i].read(&mut buffer) {
                Ok(0) => {
                    println!("Client disconnected");
                    remove_client = true;
                }
                Ok(bytes_read) => {
                    if bytes_read > 0 {
                        let data = &buffer[..bytes_read];
                        println!("Received {} bytes: {:?}", bytes_read, data);

                        if let Err(e) = clients[i].write_all(data) {
                            eprintln!("Write error: {}", e);
                            remove_client = true;
                        }
                    }
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        eprintln!("Read error: {}", e);
                        remove_client = true;
                    }
                }
            }

            if remove_client {
                clients.remove(i);
            } else {
                i += 1;
            }
        }

        // Prevent 100% CPU spinning
        std::thread::sleep(Duration::from_millis(5));
    }
}