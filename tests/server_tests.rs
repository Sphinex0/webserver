use server::config::{ServerConfig, RouteConfig};
use server::server::Server;
use std::thread;
use std::time::Duration;
use std::net::TcpStream;
use std::io::{Write, Read};

// Helper to find a free port
fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn test_payload_too_large() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();

    // 1. Configure Server with small body limit (e.g., 10 bytes)
    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    config.client_max_body_size = 10; // Very small limit
    
    // Default route needed to match "/"
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    config.routes.push(route);

    // 2. Start Server in a thread
    let _server_handle = thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });

    // Give server time to start
    thread::sleep(Duration::from_millis(100));

    // 3. Connect and send large request
    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    // Body is "123456789012345" (15 bytes) > 10 bytes
    let body = "123456789012345"; 
    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        host, port, body.len(), body
    );

    stream.write_all(request.as_bytes()).expect("Failed to write request");

    // 4. Read Response
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer).expect("Failed to read response");

    // 5. Assert 413 Payload Too Large
    assert!(buffer.contains("413 Payload Too Large"), "Response should be 413, got: {}", buffer);
    
    // Note: The server thread will likely be killed when the test process ends.
}

#[test]
fn test_payload_within_limit() {
    let port = get_free_port();
    let host = "127.0.0.1".to_string();

    // 1. Configure Server with limit 20
    let mut config = ServerConfig::default();
    config.host = host.clone();
    config.ports = vec![port];
    config.client_max_body_size = 20;
    
    let mut route = RouteConfig::default();
    route.path = "/".to_string();
    // Default file to serve so we don't get 404/403 for valid request
    // We expect 200 OK or 404 Not Found (if file missing) but NOT 413.
    // Let's rely on queue_error 404 for missing file if GET, but this is POST.
    // Our server implementation handles POST generically by clearing buffer or echoing?
    // "4. POST and DELETE logic would go here" -> it does nothing but reregister.
    // So it might timeout or close connection empty?
    // Let's use GET? NO, GET usually has no body.
    // Let's use POST. If it passes size check, it goes to `process_request`.
    // `process_request` doesn't implement POST, so it reregisters interest.
    // The connection will stay open.
    // So `read_to_string` will hang.
    
    // Workaround: We expect NO 413.
    // But we need a response to verify.
    // Let's trigger a 405 Method Not Allowed by sending POST to a GET-only route.
    // If it passes size check, it hits 405 check.
    // If it fails size check, it hits 413 check (which is earlier in logic? No, let's check order).
    
    // Logic order in server.rs:
    // 1. Check Size (queue 413) -> Break.
    // ...
    // Process Request:
    // 1. Find Route.
    // 2. Check Method. (queue 405).
    
    // So if we send oversize POST, we get 413.
    // If we send valid size POST to GET-only route, we get 405.
    
    route.methods = vec!["GET".to_string()]; // Allow only GET
    config.routes.push(route);

    // 2. Start Server
    thread::spawn(move || {
        let mut server = Server::new(vec![config]).unwrap();
        server.run().unwrap();
    });
    thread::sleep(Duration::from_millis(100));

    // 3. Connect
    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).expect("Failed to connect");
    
    let body = "12345"; // 5 bytes < 20
    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        host, port, body.len(), body
    );

    stream.write_all(request.as_bytes()).expect("Failed to write request");

    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).expect("Failed to read response");
    let response = String::from_utf8_lossy(&buffer[..n]);

    // 4. Assert 405 Method Not Allowed (proving it passed the 413 check)
    assert!(response.contains("405 Method Not Allowed"), "Response should be 405 (passed size check), got: {}", response);
}
