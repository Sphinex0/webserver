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
fn test_file_upload() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let upload_dir = "tests/uploads";

    // Clean up potentially previous runs
    // (We don't know the filenames, so maybe just ensure dir exists)
    let _ = fs::create_dir_all(upload_dir);

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    
    let mut route = RouteConfig::default();
    route.path = "/upload".to_string();
    route.root = upload_dir.to_string();
    route.methods = vec!["POST".to_string()];
    config.routes.push(route);

    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let body = "Hello, Upload!";
    let request = format!(
        "POST /upload HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        host, port, body.len(), body
    );

    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("201 Created"));
    assert!(response.contains("Location:"));

    // Verify file creation
    // We expect a file in tests/uploads starting with "upload_" and ending with ".txt"
    let paths = fs::read_dir(upload_dir).unwrap();
    let mut found = false;
    for path in paths {
        let path = path.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) == Some("txt") {
            let content = fs::read_to_string(&path).unwrap();
            if content == body {
                found = true;
                // cleanup
                fs::remove_file(path).unwrap();
                break;
            }
        }
    }
    assert!(found, "Uploaded file not found or content mismatch");
}
