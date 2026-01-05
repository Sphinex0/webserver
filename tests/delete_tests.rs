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
fn test_delete_file_success() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/delete_files".to_string();
    
    // Create specific file to verify deletion
    let file_path = format!("{}/file_success.txt", root);
    fs::write(&file_path, "delete me").unwrap();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    route.root = root.clone();
    route.methods = vec!["DELETE".to_string()];
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let request = format!("DELETE /file_success.txt HTTP/1.1\r\nHost: {}:{}\\r\\n\r\n", host, port);
    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("204 No Content"));
    assert!(!std::path::Path::new(&file_path).exists(), "File should be deleted");
}

#[test]
fn test_delete_directory_forbidden() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/delete_files".to_string();
    let dir_path = format!("{}/protected_dir", root);
    fs::create_dir_all(&dir_path).unwrap();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    route.root = root.clone();
    route.methods = vec!["DELETE".to_string()];
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let request = format!("DELETE /protected_dir HTTP/1.1\r\nHost: {}:{}\\r\\n\r\n", host, port);
    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("403 Forbidden"));
    assert!(std::path::Path::new(&dir_path).exists(), "Directory should NOT be deleted");
}

#[test]
fn test_delete_not_found() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let root = "tests/delete_files".to_string();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    route.root = root.clone();
    route.methods = vec!["DELETE".to_string()];
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let request = format!("DELETE /non_existent.txt HTTP/1.1\r\nHost: {}:{}\\r\\n\r\n", host, port);
    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("404 Not Found"));
}