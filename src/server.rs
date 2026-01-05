use std::{
    collections::HashMap,
    fs::File,
    io::{self, ErrorKind, Read, Write},
    sync::Arc,
    time::SystemTime,
};

use mio::{
    event::Event,
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};

use crate::{
    config::ServerConfig,
    http_parser::{HttpRequest, ParsingState}, utils::multipart,
};

pub struct HttpConnection {
    stream: TcpStream,
    request: HttpRequest,
    write_buffer: Vec<u8>,
    is_closing: bool,
    // Possible configurations for this connection (same Host:Port, different server_name)
    candidates: Vec<Arc<ServerConfig>>,
}

impl HttpConnection {
    fn new(stream: TcpStream, candidates: Vec<Arc<ServerConfig>>) -> Self {
        Self {
            stream,
            request: HttpRequest::new(),
            write_buffer: Vec::new(),
            is_closing: false,
            candidates,
        }
    }

    /// Resolves the correct configuration based on the Host header.
    /// Defaults to the first candidate (or the one marked default_server) if no match.
    pub fn resolve_config(&self) -> Arc<ServerConfig> {
        if let Some(host_header) = self.request.headers.get("host") {
            // Host header might be "example.com:8080", we usually just care about the name "example.com"
            // but strict matching might require checking the port too.
            // For now, let's split off the port if present.
            let hostname = host_header.split(':').next().unwrap_or("");

            for config in &self.candidates {
                if config.server_name == hostname {
                    return Arc::clone(config);
                }
            }
        }

        // If no match found, find the default_server, or fallback to the first one
        for config in &self.candidates {
            if config.default_server {
                return Arc::clone(config);
            }
        }

        // Fallback to the first one
        Arc::clone(&self.candidates[0])
    }
}

pub struct Server {
    poll: Poll,
    // Token -> (Listener, List of Configs for this port)
    listeners: HashMap<Token, (TcpListener, Vec<Arc<ServerConfig>>)>,
    connections: HashMap<Token, HttpConnection>,
    next_token: usize,
}

impl Server {
    pub fn new(configs: Vec<ServerConfig>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let mut listeners = HashMap::new();
        let mut next_token = 1;

        // Group configs by (Host, Port)
        let mut groups: HashMap<(String, u16), Vec<Arc<ServerConfig>>> = HashMap::new();

        for config in configs {
            let shared_config = Arc::new(config);
            for port in &shared_config.ports {
                let key = (shared_config.host.clone(), *port);
                groups
                    .entry(key)
                    .or_default()
                    .push(Arc::clone(&shared_config));
            }
        }

        for ((host, port), config_list) in groups {
            let addr_str = format!("{}:{}", host, port);
            let addr = addr_str.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid address: {}", addr_str),
                )
            })?;

            let mut listener = TcpListener::bind(addr)?;
            let token = Token(next_token);

            poll.registry()
                .register(&mut listener, token, Interest::READABLE)?;

            listeners.insert(token, (listener, config_list));
            next_token += 1;
            println!("Listening on {}", addr_str);
        }

        Ok(Server {
            poll,
            listeners,
            connections: HashMap::new(),
            next_token: 1000,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut events = Events::with_capacity(1024);
        loop {
            self.poll.poll(&mut events, None)?;
            for event in events.iter() {
                let token = event.token();

                // 1. Check if token belongs to one of our listeners
                if self.listeners.contains_key(&token) {
                    self.handle_listener_event(token)?;
                } else {
                    // 2. Otherwise handle as a client connection
                    self.handle_connection_event(token, event)?;
                }
            }
        }
    }

    fn handle_listener_event(&mut self, token: Token) -> io::Result<()> {
        // Get the specific listener and its candidates
        let (listener, candidates) = self.listeners.get(&token).unwrap();
        // We can't clone candidates inside the match because we borrow listener.
        // But we can clone the Arc list before looping or inside the loop if we are careful.
        // Actually, Vec<Arc<...>> is cheap to clone.
        // let candidates_clone = candidates.clone();

        loop {
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    let client_token = Token(self.next_token);
                    self.next_token += 1;

                    self.poll
                        .registry()
                        .register(&mut stream, client_token, Interest::READABLE)?;

                    // Create connection with the candidates
                    let conn = HttpConnection::new(stream, candidates.clone());
                    self.connections.insert(client_token, conn);
                    println!("Accepted {} on listener token {:?}", addr, token);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn handle_connection_event(&mut self, token: Token, event: &Event) -> io::Result<()> {
        let conn = match self.connections.get_mut(&token) {
            Some(c) => c,
            None => return Ok(()),
        };

        if event.is_readable() {
            let mut stack_buf = [0u8; 1024]; // Increased buffer size for efficiency
            loop {
                match conn.stream.read(&mut stack_buf) {
                    Ok(0) => {
                        conn.is_closing = true;
                        break;
                    }
                    Ok(n) => {
                        conn.request.append_data(&stack_buf[..n]);
                        let parse_result = conn.request.parse();
                        println!("Parse result: {:?}", parse_result);
                        match parse_result {
                            Ok(ParsingState::Body(_)) | Ok(ParsingState::Complete) => {
                                // Check Content-Length as soon as headers are available (or body accumulation starts)
                                // We need to resolve config now to check limits
                                let config = conn.resolve_config();

                                let max_size = config.client_max_body_size;

                                // Check Content-Length Header
                                if let Some(cl_str) = conn.request.headers.get("content-length") {
                                    if let Ok(cl) = cl_str.parse::<usize>() {
                                        if cl > max_size {
                                            Self::queue_error(
                                                conn,
                                                self.poll.registry(),
                                                token,
                                                413,
                                            );
                                            break;
                                        }
                                    }
                                }

                                // Also check actual buffered size just in case (defense in depth)
                                if conn.request.body.len() + conn.request.buffer.len() > max_size {
                                    Self::queue_error(conn, self.poll.registry(), token, 413);
                                    break;
                                }

                                if conn.request.state == ParsingState::Complete {
                                    Self::process_request(conn, self.poll.registry(), token)?;
                                    conn.request.clear();
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => {
                        conn.is_closing = true;
                        break;
                    }
                }
            }
        }

        if event.is_writable() {
            if !conn.write_buffer.is_empty() {
                // We need to handle potential partial writes
                match conn.stream.write(&conn.write_buffer) {
                    Ok(bytes_written) => {
                        conn.write_buffer.drain(..bytes_written);
                        if conn.write_buffer.is_empty() {
                            self.poll.registry().reregister(
                                &mut conn.stream,
                                token,
                                Interest::READABLE,
                            )?;
                        }
                        // Check pipeline processing
                        if conn.write_buffer.is_empty() && !conn.request.buffer.is_empty() {
                            match conn.request.parse() {
                                Ok(ParsingState::Complete) => {
                                    Self::process_request(conn, self.poll.registry(), token)?;
                                    conn.request.clear();
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => {
                        conn.is_closing = true;
                    }
                }
            }
        }

        if conn.is_closing && conn.write_buffer.is_empty() {
            println!("drop");
            self.connections.remove(&token);
        }

        Ok(())
    }

    fn generate_autoindex_html(dir_path: &str, req_path: &str) -> io::Result<String> {
        let entries = std::fs::read_dir(dir_path)?;
        let mut html = String::from("<!DOCTYPE html><html><head><title>Index</title></head><body>");
        html.push_str(&format!("<h1>Index of {}</h1><ul>", req_path));

        // Add parent directory link if not root
        if req_path != "/" {
            let parent_path = std::path::Path::new(req_path)
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .to_str()
                .unwrap_or("/");
            html.push_str(&format!("<li><a href=\"{}\">../</a></li>", parent_path));
        }

        let mut entries_vec = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry {
                entries_vec.push(entry);
            }
        }
        // Sort entries by name
        entries_vec.sort_by_key(|e| e.file_name());

        for entry in entries_vec {
            if let Ok(file_name) = entry.file_name().into_string() {
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let display_name = if is_dir {
                    format!("{}/", file_name)
                } else {
                    file_name.clone()
                };

                // Construct relative link
                // If req_path ends with /, append file_name
                // Else append /file_name
                let link = if req_path.ends_with('/') {
                    format!("{}{}", req_path, file_name)
                } else {
                    format!("{}/{}", req_path, file_name)
                };

                html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    link, display_name
                ));
            }
        }

        html.push_str("</ul></body></html>");
        Ok(html)
    }

    fn queue_error(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        status_code: u16,
    ) {
        let error_msg = match status_code {
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            _ => "Internal Server Error",
        };

        // Resolve config to check for custom error pages
        let config = conn.resolve_config();

        let body = if let Some(path) = config.error_pages.get(&status_code) {
            std::fs::read_to_string(path)
                .unwrap_or_else(|_| format!("<h1>{} {}</h1>", status_code, error_msg))
        } else {
            format!("<h1>{} {}</h1>", status_code, error_msg)
        };

        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status_code,
            error_msg,
            body.len(),
            body
        );

        conn.write_buffer.extend_from_slice(response.as_bytes());
        conn.is_closing = true;

        let _ = registry.reregister(
            &mut conn.stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        );
    }

    fn process_request(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
    ) -> io::Result<()> {
        let path = conn.request.path.clone();
        let method = conn.request.method.clone();

        // Resolve the correct configuration for this request
        let config = conn.resolve_config();

        // 1. Longest Prefix Match to find the route
        let route = match config.find_route(&path) {
            Some(r) => r,
            None => {
                Self::queue_error(conn, registry, token, 404);
                return Ok(());
            }
        };

        // 2. Check Methods
        if !route.methods.contains(&method) {
            Self::queue_error(conn, registry, token, 405);
            return Ok(());
        }

        // 3. Handle Redirection
        if let Some(redirection_url) = &route.redirection {
            let response = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                redirection_url
            );
            conn.write_buffer.extend_from_slice(response.as_bytes());
            conn.is_closing = true;

            // Finalize
            registry.reregister(
                &mut conn.stream,
                token,
                Interest::READABLE.add(Interest::WRITABLE),
            )?;
            return Ok(());
        }

        // 4. Method Dispatch
        match method.as_str() {
            "GET" => Self::handle_get(conn, registry, token, route, &path)?,
            "POST" => Self::handle_post(conn, registry, token, route)?,
            "DELETE" => Self::handle_delete(conn, registry, token, route, &path)?,
            _ => {
                // Should have been caught by method check, but safe fallback
                Self::queue_error(conn, registry, token, 405);
                return Ok(());
            }
        }

        registry.reregister(
            &mut conn.stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        )?;

        Ok(())
    }

    fn handle_delete(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        route: &crate::config::RouteConfig,
        path: &str,
    ) -> io::Result<()> {
        let mut full_path;
        if path == route.path && !route.default_file.is_empty() {
            full_path = format!("{}/", route.root);
        } else {
            full_path = format!(
                "{}/{}",
                route.root,
                path.strip_prefix(&route.path).unwrap_or("")
            );
        }
        
        // Clean up double slashes just in case
        full_path = full_path.replace("//", "/");
        
        println!("Deleting file: {}", full_path);
        
        if std::path::Path::new(&full_path).exists() {
            if std::fs::metadata(&full_path).map(|m| m.is_dir()).unwrap_or(false) {
                 // Directory deletion forbidden
                 Self::queue_error(conn, registry, token, 403);
            } else {
                match std::fs::remove_file(&full_path) {
                    Ok(_) => {
                        let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
                        conn.write_buffer.extend_from_slice(response.as_bytes());
                        conn.is_closing = true;
                    }
                    Err(_) => {
                        Self::queue_error(conn, registry, token, 403); // Forbidden or Locked
                    }
                }
            }
        } else {
            Self::queue_error(conn, registry, token, 404);
        }
        Ok(())
    }

    fn handle_post(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        route: &crate::config::RouteConfig,
    ) -> io::Result<()> {
        let content_type = conn
            .request
            .headers
            .get("content-type")
            .map(|s| s.as_str())
            .unwrap_or("application/octet-stream");

        // Check for multipart
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .map(|b| b.trim())
            .unwrap_or("");
        dbg!(&conn.request.headers);
        dbg!(String::from_utf8(conn.request.body.clone()).unwrap());
        if boundary != "" {
            if !boundary.is_empty() {
                println!("Handling Multipart Upload with boundary: {}", boundary);
                let parts = multipart::parse_multipart(&conn.request.body, boundary);
                let mut uploaded_count = 0;

                let mut uploaded_files = Vec::new();

                for part in parts {
                    // Determine filename behavior:
                    // 1. Missing 'filename' attribute -> Skip (likely a regular form field)
                    // 2. Empty 'filename' value ("") -> Generate a unique name
                    // 3. Provided 'filename' -> Use it
                    let mut filename = match part.filename {
                        None => continue, // Case 1: Skip
                        Some(f) if f.is_empty() => {
                            // Case 2: Generate
                            let ext = part.content_type.as_deref()
                                .map(Self::get_ext_from_content_type)
                                .unwrap_or(".bin");
                            format!("upload_{}{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(), ext)
                        }
                        Some(f) => f, // Case 3: Use provided
                    };

                    // Construct path and sanitize
                    let mut safe_filename = std::path::Path::new(&filename)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or(filename.clone()); 

                    let mut upload_path = std::path::PathBuf::from(&route.root);
                    upload_path.push(&safe_filename);

                    // Handle duplicates by renaming (file.txt -> file(1).txt)
                    if upload_path.exists() {
                        let stem = std::path::Path::new(&safe_filename)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&safe_filename)
                            .to_string();
                        let extension = std::path::Path::new(&safe_filename)
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|e| format!(".{}", e))
                            .unwrap_or_default();
                        
                        let mut counter = 1;
                        while upload_path.exists() {
                            safe_filename = format!("{}({}){}", stem, counter, extension);
                            upload_path = std::path::PathBuf::from(&route.root);
                            upload_path.push(&safe_filename);
                            counter += 1;
                        }
                        // Update filename to the resolved unique name
                        filename = safe_filename; 
                    }

                    // Ensure parent directory exists
                    if let Some(parent) = upload_path.parent() {
                        if !parent.exists() {
                            if let Err(_) = std::fs::create_dir_all(parent) {
                                Self::queue_error(conn, registry, token, 500);
                                return Ok(());
                            }
                        }
                    }

                    println!("Saving multipart file: {:?}", upload_path);
                    match File::create(&upload_path) {
                        Ok(mut file) => {
                            if let Err(_) = file.write_all(&part.body) {
                                Self::queue_error(conn, registry, token, 500);
                                return Ok(());
                            }
                            uploaded_count += 1;
                            uploaded_files.push(filename);
                        }
                        Err(_) => {
                            Self::queue_error(conn, registry, token, 500);
                            return Ok(());
                        }
                    }
                }

                let response_body = format!("Uploaded {} files successfully:\n{}", uploaded_count, uploaded_files.join("\n"));
                let response = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                conn.write_buffer.extend_from_slice(response.as_bytes());
                return Ok(());
            }
        }

        // Fallback to raw body upload (previous behavior)
        let extension = Self::get_ext_from_content_type(content_type);

        // Generate a unique filename
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos(); // Use nanos for better uniqueness
        let filename = format!("upload_{}{}", timestamp, extension);

        // Construct the full path using PathBuf for safety
        let mut upload_path = std::path::PathBuf::from(&route.root);

        // If the route has a dedicated upload directory defined (not part of RouteConfig yet, but we can assume root for now or check if root is a dir)
        // For now, we write to route.root.
        // Ideally, we might want to preserve the request path structure, but standard POST uploads often go to a specific store.
        // Let's stick to the user's logic of "root + generated name".

        upload_path.push(filename);

        println!("Uploading to: {:?}", upload_path);

        // Ensure parent directory exists (though route.root should exist)
        if let Some(parent) = upload_path.parent() {
            if !parent.exists() {
                // Try to create it? Or fail?
                // If route.root points to a non-existent dir, we might want to create it.
                if let Err(_) = std::fs::create_dir_all(parent) {
                    Self::queue_error(conn, registry, token, 500);
                    return Ok(());
                }
            }
        }

        match File::create(&upload_path) {
            Ok(mut file) => match file.write_all(&conn.request.body) {
                Ok(_) => {
                    let response_body = "File uploaded successfully";
                    let response = format!(
                            "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nLocation: {}\r\nConnection: keep-alive\r\n\r\n{}",
                            response_body.len(),
                            upload_path.file_name().unwrap().to_string_lossy(),
                            response_body
                        );
                    conn.write_buffer.extend_from_slice(response.as_bytes());
                }
                Err(_) => {
                    Self::queue_error(conn, registry, token, 500);
                }
            },
            Err(_) => {
                // Failed to create file (permissions, etc.)
                Self::queue_error(conn, registry, token, 500);
            }
        }
        Ok(())
    }

    fn get_ext_from_content_type(content_type: &str) -> &str {
        match content_type {
            "application/json" => ".json",
            "application/pdf" => ".pdf",
            "application/xml" => ".xml",
            "application/zip" => ".zip",
            "audio/mpeg" => ".mp3",
            "image/gif" => ".gif",
            "image/jpeg" => ".jpg",
            "image/png" => ".png",
            "image/svg+xml" => ".svg",
            "image/webp" => ".webp",
            "text/css" => ".css",
            "text/html" => ".html",
            "text/javascript" => ".js",
            "text/plain" => ".txt",
            "video/mp4" => ".mp4",
            _ => ".bin",
        }
    }

    fn handle_get(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        route: &crate::config::RouteConfig,
        path: &str,
    ) -> io::Result<()> {
        let mut full_path;
        if path == route.path && !route.default_file.is_empty() {
            full_path = format!("{}/", route.root);
        } else {
            full_path = format!(
                "{}/{}",
                route.root,
                path.strip_prefix(&route.path).unwrap_or("")
            );
        }

        // Clean up double slashes just in case
        full_path = full_path.replace("//", "/");

        let metadata = std::fs::metadata(&full_path);
        match metadata {
            Ok(md) => {
                let mut serve_file = false;
                if md.is_dir() {
                    // Directory handling
                    let index_path = format!("{}/{}", full_path, route.default_file);
                    if !route.default_file.is_empty() && std::path::Path::new(&index_path).exists()
                    {
                        // Serve index file
                        full_path = index_path;
                        serve_file = true;
                    } else if route.autoindex {
                        // Serve Autoindex
                        match Self::generate_autoindex_html(&full_path, &path) {
                            Ok(html) => {
                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                                    html.len(),
                                    html
                                );
                                conn.write_buffer.extend_from_slice(response.as_bytes());
                            }
                            Err(_) => {
                                Self::queue_error(conn, registry, token, 500);
                            }
                        }
                    } else {
                        // Forbidden (Directory listing disabled and no index file)
                        Self::queue_error(conn, registry, token, 403);
                    }
                } else {
                    serve_file = true;
                }

                if serve_file {
                    println!("Serving file: {}", full_path);
                    match std::fs::read(&full_path) {
                        Ok(content) => {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                content.len()
                            );
                            conn.write_buffer.extend_from_slice(response.as_bytes());
                            conn.write_buffer.extend_from_slice(&content);
                        }
                        Err(_) => {
                            // Could happen if file permissions deny read
                            Self::queue_error(conn, registry, token, 403);
                        }
                    }
                }
            }
            Err(_) => {
                Self::queue_error(conn, registry, token, 404);
            }
        }
        Ok(())
    }
}
