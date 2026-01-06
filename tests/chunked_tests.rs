use server::config::{ServerConfig, RouteConfig};
use server::server::Server;
use std::thread;
use std::time::Duration;
use std::net::TcpStream;
use std::io::{Write, Read};
use std::fs;

fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn test_chunked_upload() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/chunked";
    
    let _ = fs::create_dir_all(root);

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/upload".to_string();
    route.root = root.to_string();
    route.methods = vec!["POST".to_string()];
    config.routes.push(route);
    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));
    
    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    // Send Headers
    let headers = format!(
        "POST /upload HTTP/1.1\r\n\
        Host: {}:{}\r\n\
        Content-Type: text/plain\r\n\
        Transfer-Encoding: chunked\r\n\
        \r\n",
        host, port
    );
    stream.write_all(headers.as_bytes()).unwrap();
    // let mut buf = [0u8; 1024];
    // // stream.read(&mut buf);
    // println!("#########");
    // println!("buuuuuuffffeeeeerrrr:{buf:?}");
    // Send chunks
    // Chunk 1: "Hello" (5 bytes)
    stream.write_all(b"5\r\nHello\r\n").unwrap();
    thread::sleep(Duration::from_millis(10));
    
    // Chunk 2: " World" (6 bytes)
    stream.write_all(b"6\r\n World\r\n").unwrap();
    thread::sleep(Duration::from_millis(10));
    
    // End chunk: 0
    stream.write_all(b"0\r\n\r\n").unwrap();

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("201 Created"));
    
    // Verify file content
    // We need to find the uploaded file. It starts with "upload_"
    let mut found = false;
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_str().unwrap().starts_with("upload_") {
            let content = fs::read_to_string(&path).unwrap();
            if content == "Hello World" {
                found = true;
                fs::remove_file(path).unwrap();
            }
        }
    }
    assert!(found, "Uploaded chunked content mismatch");
}
