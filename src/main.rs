use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

use server::error::*;

const ADDRESS: &str = "127.0.0.1:8080";
const BUFFER_SIZE: usize = 512;

fn handle_client(mut stream: TcpStream) {
    println!("New connection from : {}", stream.peer_addr().unwrap());
    let mut buffer = [0; BUFFER_SIZE];

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Client disconnected");
                break;
            }
            Ok(bytes_read) => {
                let data = &buffer[..bytes_read];
                println!("Received {} bytes: {:?}", bytes_read, data);

                if stream.write_all(data).is_err() {
                    println!("Failed to flush stream.");
                    break;
                }
            }
            Err(e) => {
                eprintln!("An error occurred: {}", e);
                break;
            }
        }
    }
}

fn main() -> Result<()> {
    let listener = TcpListener::bind(ADDRESS)?;
    println!("Server listening on http://{}", ADDRESS);

    for stream in listener.incoming(){
        match stream{
            Ok(stream)=>{
                handle_client(stream);
            }
            Err(e)=>{
                eprintln!("Connection failed: {}",e);
            }
        }
    }

    Ok(())
}
