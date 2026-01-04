use server::config::{ServerConfig, RouteConfig};
use server::server::Server;
use std::thread;
use std::time::Duration;
use std::net::TcpStream;
use std::io::{Write, Read};

fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn test_autoindex_generation() {
    let _ = std::fs::create_dir_all("tests/www_autoindex/sub");
    let _ = std::fs::write("tests/www_autoindex/file1.txt", "content");
    let _ = std::fs::write("tests/www_autoindex/sub/file2.txt", "content");

    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/www_autoindex".to_string();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    route.root = root.clone();
    route.autoindex = true;
    route.default_file = "".to_string(); // Disable default file to force autoindex
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    // Use simple string concat to avoid issues
    let request = format!("GET / HTTP/1.1\r\nHost: {}:{}\r\n\r\n", host, port);
    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("200 OK"));
    assert!(response.contains("Index of /"));
    assert!(response.contains("file1.txt"));
    assert!(response.contains("sub/"));
}

#[test]
fn test_autoindex_subdirectory() {
    let _ = std::fs::create_dir_all("tests/www_autoindex/sub");
    let _ = std::fs::write("tests/www_autoindex/sub/file2.txt", "content");

    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/www_autoindex".to_string();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    route.root = root.clone();
    route.autoindex = true;
    route.default_file = "".to_string();
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let request = format!("GET /sub/ HTTP/1.1\r\nHost: {}:{}\r\n\r\n", host, port);
    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("200 OK"));
    assert!(response.contains("Index of /sub/"));
    assert!(response.contains("file2.txt"));
}