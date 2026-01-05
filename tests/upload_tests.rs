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

#[test]
fn test_multipart_upload() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let upload_dir = "tests/uploads_multipart";
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
    
    let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
    let body = format!(
        "--{0}\r\n\
         Content-Disposition: form-data; name=\"file1\"; filename=\"test1.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         Content of file 1\r\n\
         --{0}\r\n\
         Content-Disposition: form-data; name=\"field1\"\r\n\
         \r\n\
         This is a regular form field, should be skipped\r\n\
         --{0}\r\n\
         Content-Disposition: form-data; name=\"file2\"; filename=\"\"\r\n\
         Content-Type: image/png\r\n\
         \r\n\
         <binary data for file 2>\r\n\
         --{0}--\r\n",
        boundary
    );

    let request = format!(
        "POST /upload HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: multipart/form-data; boundary={}\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        host, port, boundary, body.len(), body
    );

    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("201 Created"));
    // Should still be 2 files, the form field part should be skipped
    assert!(response.contains("Uploaded 2 files successfully"));

    // Verify files
    let mut file1_found = false;
    let mut file2_found = false;
    let mut field_found = false;
    for entry in fs::read_dir(upload_dir).unwrap() {
        let path = entry.unwrap().path();
        let content = fs::read_to_string(&path).unwrap();
        if content == "Content of file 1" {
            file1_found = true;
        } else if content == "<binary data for file 2>" {
            file2_found = true;
        } else if content.contains("regular form field") {
            field_found = true;
        }
        fs::remove_file(path).unwrap();
    }
    assert!(file1_found, "File 1 not found");
    assert!(file2_found, "File 2 (generated name) not found");
    assert!(!field_found, "Regular form field should NOT have been saved as a file");
}

#[test]
fn test_duplicate_upload_renaming() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let upload_dir = "tests/uploads_duplicates";
    let _ = fs::create_dir_all(upload_dir);
    
    // Pre-create file to force collision
    let initial_content = "Original Content";
    let path = format!("{}/duplicate.txt", upload_dir);
    fs::write(&path, initial_content).unwrap();

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
    
    let boundary = "----BoundaryXY";
    let body = format!(
        "--{0}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"duplicate.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         New Content 1\r\n\
         --{0}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"duplicate.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         New Content 2\r\n\
         --{0}--\r\n",
        boundary
    );

    let request = format!(
        "POST /upload HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: multipart/form-data; boundary={}\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        host, port, boundary, body.len(), body
    );

    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response:\n{}", response);

    assert!(response.contains("201 Created"));
    // Check if response lists filenames
    assert!(response.contains("duplicate(1).txt"));
    assert!(response.contains("duplicate(2).txt"));

    // Verify files on disk
    assert_eq!(fs::read_to_string(format!("{}/duplicate.txt", upload_dir)).unwrap(), "Original Content");
    assert_eq!(fs::read_to_string(format!("{}/duplicate(1).txt", upload_dir)).unwrap(), "New Content 1");
    assert_eq!(fs::read_to_string(format!("{}/duplicate(2).txt", upload_dir)).unwrap(), "New Content 2");
    
    // Cleanup
    fs::remove_dir_all(upload_dir).unwrap();
}
