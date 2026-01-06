use server::config::{ServerConfig, RouteConfig};
use server::server::Server;
use std::thread;
use std::time::Duration;
use std::net::TcpStream;
use std::io::{Write, Read};
use std::fs;
use std::path::Path;

fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn test_image_upload_1_png() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();
    let upload_dir = "tests/uploads_image";
    let _ = fs::create_dir_all(upload_dir);
    
    let source_path = "1.png";
    assert!(Path::new(source_path).exists(), "1.png must exist for this test");
    let image_data = fs::read(source_path).expect("Failed to read 1.png");
    let image_len = image_data.len();

    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    config.client_max_body_size = 20 * 1024 * 1024;
    
    let mut route = RouteConfig::default();
    route.path = "/upload".to_string();
    route.root = upload_dir.to_string();
    route.methods = vec!["POST".to_string()];
    config.routes.push(route);

    thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(200));

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let boundary = "----WebKitFormBoundaryImageUpload";
    
    let mut part_header = String::new();
    part_header.push_str(&format!("--{}\r\n", boundary));
    part_header.push_str("Content-Disposition: form-data; name=\"file\"; filename=\"1.png\"\r\n");
    part_header.push_str("Content-Type: image/png\r\n\r\n");
    
    let mut part_footer = String::new();
    part_footer.push_str(&format!("\r\n--{}--\r\n", boundary));
    
    let body_len = part_header.len() + image_data.len() + part_footer.len();

    let mut request_headers = String::new();
    request_headers.push_str("POST /upload HTTP/1.1\r\n");
    request_headers.push_str(&format!("Host: {}:{}\r\n", host, port));
    request_headers.push_str(&format!("Content-Type: multipart/form-data; boundary={}\r\n", boundary));
    request_headers.push_str(&format!("Content-Length: {}\r\n\r\n", body_len));
    
    stream.write_all(request_headers.as_bytes()).unwrap();
    stream.write_all(part_header.as_bytes()).unwrap();
    stream.write_all(&image_data).unwrap();
    stream.write_all(part_footer.as_bytes()).unwrap();
    stream.flush().unwrap();

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    println!("Response Header:\n{}", response);

    assert!(response.contains("201 Created"));
    
    let mut found_path = None;
    for entry in fs::read_dir(upload_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_string_lossy().starts_with("1") {
            found_path = Some(path);
            break;
        }
    }
    
    let path = found_path.expect("Uploaded file not found on disk");
    let uploaded_data = fs::read(&path).expect("Failed to read uploaded file");
    
    println!("Image size: {}, Uploaded size: {}", image_len, uploaded_data.len());
    assert_eq!(uploaded_data.len(), image_len, "Uploaded size mismatch");
    assert_eq!(uploaded_data, image_data, "Content mismatch");
    
    fs::remove_dir_all(upload_dir).unwrap();
}