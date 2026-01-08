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
    http_parser::{HttpRequest, ParsingState},
    utils::multipart,
};

enum BodyHandler {
    None,
    Buffer(Vec<u8>), // For small payloads or when buffering is required (e.g. headers incomplete)
    File { file: File, path: std::path::PathBuf }, // For streaming raw uploads
    Multipart { 
        parser: multipart::MultipartParser, 
        current_file: Option<File>, 
        current_filename: Option<String>,
        uploaded_files: Vec<String>,
        upload_count: usize,
    },
}

pub struct HttpConnection {
    stream: TcpStream,
    request: HttpRequest,
    write_buffer: Vec<u8>,
    is_closing: bool,
    candidates: Vec<Arc<ServerConfig>>,
    body_handler: BodyHandler,
    current_route: Option<crate::config::RouteConfig>, // Cache resolved route
}

impl HttpConnection {
    fn new(stream: TcpStream, candidates: Vec<Arc<ServerConfig>>) -> Self {
        Self {
            stream,
            request: HttpRequest::new(),
            write_buffer: Vec::new(),
            is_closing: false,
            candidates,
            body_handler: BodyHandler::None,
            current_route: None,
        }
    }

    /// Resolves the correct configuration based on the Host header.
    pub fn resolve_config_static(headers: &HashMap<String, String>, candidates: &[Arc<ServerConfig>]) -> Arc<ServerConfig> {
        if let Some(host_header) = headers.get("host") {
            let hostname = host_header.split(':').next().unwrap_or("");
            for config in candidates {
                if config.server_name == hostname {
                    return Arc::clone(config);
                }
            }
        }
        for config in candidates {
            if config.default_server {
                return Arc::clone(config);
            }
        }
        Arc::clone(&candidates[0])
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
            let mut stack_buf = [0u8; 4096]; // 4KB buffer for fairness
            let mut reads = 0;
            const MAX_READS: usize = 16;
            
            loop {
                match conn.stream.read(&mut stack_buf) {
                    Ok(0) => {
                        conn.is_closing = true;
                        break;
                    }
                    Ok(n) => {
                        // println!("DEBUG: Read {} bytes", n);
                        conn.request.append_data(&stack_buf[..n]);
                        
                        // Parse Loop
                        loop {
                            let parse_result = conn.request.parse();
                            match parse_result {
                                Ok(ParsingState::Body { remaining }) | Ok(ParsingState::ChunkBody { remaining }) => {
                                    // println!("DEBUG: Body/Chunk State. Remaining: {}", remaining);
                                    // 1. Initialize Handler if None
                                    if matches!(conn.body_handler, BodyHandler::None) {
                                        // Resolve Route Early
                                        let config = HttpConnection::resolve_config_static(&conn.request.headers, &conn.candidates);
                                        let route = config.find_route(&conn.request.path).cloned(); // Clone route config
                                        conn.current_route = route.clone();
                                        
                                        if let Some(r) = &route {
                                            // Check limits
                                            let max_size = config.client_max_body_size;
                                            // Determine Content-Length if possible
                                            let content_len = conn.request.headers.get("content-length")
                                                .and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                                                
                                            if content_len > max_size {
                                                Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 413, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                break;
                                            }
                                            
                                            
                                            // Decide Strategy
                                            let method = conn.request.method.as_str();
                                            let content_type = conn.request.headers.get("content-type").map(|s| s.as_str()).unwrap_or("");
                                            
                                            if method == "POST" && !content_type.starts_with("multipart/form-data") {
                                                // Raw Upload -> Stream to File
                                                // ... (existing raw upload logic)
                                                let ext = Self::get_ext_from_content_type(content_type);
                                                let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
                                                let filename = format!("upload_{}{}", timestamp, ext);
                                                let mut path = std::path::PathBuf::from(&r.root);
                                                path.push(filename);
                                                
                                                if let Some(p) = path.parent() { std::fs::create_dir_all(p).ok(); }
                                                
                                                println!("Streaming raw upload to: {:?}", path);
                                                match File::create(&path) {
                                                    Ok(f) => conn.body_handler = BodyHandler::File { file: f, path },
                                                    Err(_) => {
                                                        Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 500, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                        break;
                                                    }
                                                }
                                            } else if method == "POST" && content_type.starts_with("multipart/form-data") {
                                                // Streaming Multipart
                                                let boundary = content_type
                                                    .split("boundary=")
                                                    .nth(1)
                                                    .map(|b| b.trim().trim_matches('"'))
                                                    .unwrap_or("");
                                                
                                                if boundary.is_empty() {
                                                     Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 400, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                     break;
                                                }

                                                println!("Initializing Multipart Stream with boundary: {}", boundary);
                                                conn.body_handler = BodyHandler::Multipart { 
                                                    parser: multipart::MultipartParser::new(boundary),
                                                    current_file: None,
                                                    current_filename: None,
                                                    uploaded_files: Vec::new(),
                                                    upload_count: 0,
                                                };
                                            } else {
                                                // Other -> Buffer (fallback)
                                                conn.body_handler = BodyHandler::Buffer(Vec::new());
                                            }
                                        } else {
                                            // No route? 404.
                                            Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 404, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                            break;
                                        }
                                    }
                                    
                                    // 2. Stream Data
                                    let available = conn.request.buffer.len();
                                    let to_consume = std::cmp::min(available, remaining);
                                    
                                    if to_consume > 0 {
                                        let data = &conn.request.buffer[..to_consume];
                                        // Pre-calculate limit to avoid borrowing conn inside match
                                        let limit = HttpConnection::resolve_config_static(&conn.request.headers, &conn.candidates).client_max_body_size;
                                        
                                        match &mut conn.body_handler {
                                            BodyHandler::File { file, .. } => {
                                                if let Err(_) = file.write_all(data) {
                                                    Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 500, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                    break;
                                                }
                                            }
                                            BodyHandler::Buffer(vec) => {
                                                if vec.len() + data.len() > limit {
                                                     Self::queue_error_static(self.poll.registry(), &mut conn.stream, token, 413, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                     break;
                                                }
                                                vec.extend_from_slice(data);
                                            }
                                            BodyHandler::Multipart { parser, current_file, current_filename, uploaded_files, upload_count } => {
                                                // Use zero-copy sink API to write parts directly to file
                                                let route = conn.current_route.as_ref().unwrap().clone();
                                                let registry = self.poll.registry();

                                                parser.process_to_sink(data, |event| {
                                                    use crate::utils::multipart::MultipartEventRef;
                                                    match event {
                                                        MultipartEventRef::StartPart(part_headers) => {
                                                            let content_disposition = part_headers.get("Content-Disposition").map(|s| s.as_str()).unwrap_or("");
                                                            let content_type = part_headers.get("Content-Type").map(|s| s.as_str());

                                                            let mut filename = None;
                                                            for part in content_disposition.split(';') {
                                                                let part = part.trim();
                                                                if part.starts_with("filename=") {
                                                                    let val = part.trim_start_matches("filename=").trim_matches('"');
                                                                    if !val.is_empty() {
                                                                        filename = Some(val.to_string());
                                                                    } else {
                                                                        let ext = content_type.map(Self::get_ext_from_content_type).unwrap_or(".bin");
                                                                        filename = Some(format!("upload_{}{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(), ext));
                                                                    }
                                                                }
                                                            }

                                                            if let Some(fname) = filename {
                                                                let safe_filename = std::path::Path::new(&fname).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(fname.clone());
                                                                let mut upload_path = std::path::PathBuf::from(&route.root);
                                                                upload_path.push(&safe_filename);

                                                                let mut final_filename = safe_filename.clone();
                                                                if upload_path.exists() {
                                                                    let stem = std::path::Path::new(&safe_filename).file_stem().and_then(|s| s.to_str()).unwrap_or(&safe_filename).to_string();
                                                                    let ext = std::path::Path::new(&safe_filename).extension().and_then(|s| s.to_str()).map(|e| format!(".{}", e)).unwrap_or_default();
                                                                    let mut c = 1;
                                                                    while upload_path.exists() {
                                                                        final_filename = format!("{}({}){}", stem, c, ext);
                                                                        upload_path = std::path::PathBuf::from(&route.root);
                                                                        upload_path.push(&final_filename);
                                                                        c += 1;
                                                                    }
                                                                }

                                                                if let Some(parent) = upload_path.parent() { std::fs::create_dir_all(parent).ok(); }

                                                                println!("Start multipart file: {:?}", upload_path);
                                                                match File::create(&upload_path) {
                                                                    Ok(f) => {
                                                                        *current_file = Some(f);
                                                                        *current_filename = Some(final_filename);
                                                                    }
                                                                    Err(_) => {
                                                                        Self::queue_error_static(registry, &mut conn.stream, token, 500, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                                        return false;
                                                                    }
                                                                }
                                                            } else {
                                                                *current_file = None;
                                                                *current_filename = None;
                                                            }
                                                            true
                                                        }
                                                        MultipartEventRef::PartData(bytes) => {
                                                            if let Some(file) = current_file {
                                                                if let Err(_) = file.write_all(bytes) {
                                                                    Self::queue_error_static(registry, &mut conn.stream, token, 500, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                                    return false;
                                                                }
                                                            }
                                                            true
                                                        }
                                                        MultipartEventRef::EndPart => {
                                                            if let Some(file) = current_file.take() {
                                                                file.sync_all().ok();
                                                                if let Some(name) = current_filename.take() {
                                                                    uploaded_files.push(name);
                                                                    *upload_count += 1;
                                                                }
                                                            }
                                                            true
                                                        }
                                                        MultipartEventRef::Finished => {
                                                            true
                                                        }
                                                        MultipartEventRef::Error(_) => {
                                                            Self::queue_error_static(registry, &mut conn.stream, token, 400, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers);
                                                            return false;
                                                        }
                                                    }
                                                });
                                            }
                                            BodyHandler::None => {} 
                                        }
                                        
                                        conn.request.consume_body(to_consume);
                                    } else {
                                        break; 
                                    }
                                }
                                Ok(ParsingState::Complete) => {
                                    // Finalize Body Handler if needed
                                    if let BodyHandler::Multipart { ref mut parser, ref mut current_file, ref mut current_filename, ref mut uploaded_files, ref mut upload_count } = conn.body_handler {
                                         let events = parser.finalize();
                                         Self::handle_multipart_events(self.poll.registry(), token, events, current_file, current_filename, uploaded_files, upload_count, &mut conn.stream, &mut conn.is_closing, &mut conn.write_buffer, &conn.candidates, &conn.request.headers, conn.current_route.as_ref().unwrap());
                                    }

                                    // Finalize Request
                                    Self::process_request(conn, self.poll.registry(), token)?;
                                    conn.request.clear();
                                    conn.body_handler = BodyHandler::None; // Reset
                                    conn.current_route = None;
                                    break;
                                }
                                Ok(_) => {
                                    break;
                                }
                                Err(e) => {
                                    // println!("DEBUG: Parser Error: {}", e);
                                    conn.is_closing = true;
                                    break;
                                }
                            }
                        }
                        
                        // Check yield condition
                        reads += 1;
                        if reads >= MAX_READS {
                             // Yield to let other connections process
                             // We re-register interest to ensure we get called again (if data remains in kernel buffer)
                             self.poll.registry().reregister(&mut conn.stream, token, Interest::READABLE).ok();
                             break;
                        }

                        // If we decided to close (e.g. error sent), stop reading
                        if conn.is_closing {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) => {
                        // println!("DEBUG: Read Error: {}", e);
                        conn.is_closing = true;
                        break;
                    }
                }
            }
        }

        if event.is_writable() {
            dbg!(&conn.write_buffer.len());
            if !conn.write_buffer.is_empty() {
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

    fn handle_multipart_events(
        registry: &mio::Registry,
        token: Token,
        events: Vec<multipart::MultipartEvent>,
        current_file: &mut Option<File>,
        current_filename: &mut Option<String>,
        uploaded_files: &mut Vec<String>,
        upload_count: &mut usize,
        stream: &mut TcpStream,
        conn_is_closing: &mut bool,
        conn_write_buffer: &mut Vec<u8>,
        candidates: &[Arc<ServerConfig>],
        headers: &HashMap<String, String>,
        route: &crate::config::RouteConfig,
    ) {
        for event in events {
            match event {
                multipart::MultipartEvent::StartPart(part_headers) => {
                    let content_disposition = part_headers.get("Content-Disposition").map(|s| s.as_str()).unwrap_or("");
                    let content_type = part_headers.get("Content-Type").map(|s| s.as_str());

                    let mut filename = None;
                    for part in content_disposition.split(';') {
                        let part = part.trim();
                        if part.starts_with("filename=") {
                            let val = part.trim_start_matches("filename=").trim_matches('"');
                            if !val.is_empty() {
                                filename = Some(val.to_string());
                            } else {
                                let ext = content_type.map(Self::get_ext_from_content_type).unwrap_or(".bin");
                                filename = Some(format!("upload_{}{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(), ext));
                            }
                        }
                    }
                    
                    if let Some(fname) = filename {
                        let safe_filename = std::path::Path::new(&fname).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(fname.clone());
                        let mut upload_path = std::path::PathBuf::from(&route.root);
                        upload_path.push(&safe_filename);
                        
                        let mut final_filename = safe_filename.clone();
                        if upload_path.exists() {
                            let stem = std::path::Path::new(&safe_filename).file_stem().and_then(|s| s.to_str()).unwrap_or(&safe_filename).to_string();
                            let ext = std::path::Path::new(&safe_filename).extension().and_then(|s| s.to_str()).map(|e| format!(".{}", e)).unwrap_or_default();
                            let mut c = 1;
                            while upload_path.exists() {
                                final_filename = format!("{}({}){}", stem, c, ext);
                                upload_path = std::path::PathBuf::from(&route.root);
                                upload_path.push(&final_filename);
                                c += 1;
                            }
                        }
                        
                        if let Some(parent) = upload_path.parent() { std::fs::create_dir_all(parent).ok(); }
                        
                        println!("Start multipart file: {:?}", upload_path);
                        match File::create(&upload_path) {
                            Ok(f) => {
                                *current_file = Some(f);
                                *current_filename = Some(final_filename);
                            }
                            Err(_) => {
                                Self::queue_error_static(registry, stream, token, 500, conn_is_closing, conn_write_buffer, candidates, headers);
                                return;
                            }
                        }
                    } else {
                        *current_file = None;
                        *current_filename = None;
                    }
                }
                multipart::MultipartEvent::PartData(bytes) => {
                    if let Some(file) = current_file {
                        if let Err(_) = file.write_all(&bytes) {
                            Self::queue_error_static(registry, stream, token, 500, conn_is_closing, conn_write_buffer, candidates, headers);
                            return;
                        }
                    }
                }
                multipart::MultipartEvent::EndPart => {
                    if let Some(file) = current_file.take() {
                        file.sync_all().ok();
                        if let Some(name) = current_filename.take() {
                            uploaded_files.push(name);
                            *upload_count += 1;
                        }
                    }
                }
                multipart::MultipartEvent::Finished => {}
                multipart::MultipartEvent::Error(_) => {
                    Self::queue_error_static(registry, stream, token, 400, conn_is_closing, conn_write_buffer, candidates, headers);
                    return;
                }
            }
        }
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

    fn queue_error_static(
        registry: &mio::Registry,
        stream: &mut TcpStream,
        token: Token,
        status_code: u16,
        conn_is_closing: &mut bool,
        conn_write_buffer: &mut Vec<u8>,
        candidates: &[Arc<ServerConfig>],
        headers: &HashMap<String, String>,
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
        let config = HttpConnection::resolve_config_static(headers, candidates);

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

        conn_write_buffer.extend_from_slice(response.as_bytes());
        *conn_is_closing = true;

        let _ = registry.reregister(
            stream,
            token,
            Interest::READABLE.add(Interest::WRITABLE),
        );
    }

    fn queue_error(
        conn: &mut HttpConnection,
        registry: &mio::Registry,
        token: Token,
        status_code: u16,
    ) {
        Self::queue_error_static(
            registry,
            &mut conn.stream,
            token,
            status_code,
            &mut conn.is_closing,
            &mut conn.write_buffer,
            &conn.candidates,
            &conn.request.headers,
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
        let config = HttpConnection::resolve_config_static(&conn.request.headers, &conn.candidates);

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
            "POST" => {
                // If it was Raw Upload, it's already done (streamed to file).
                // We just need to send response.
                // We consume the body handler state to avoid borrow issues
                let mut handler = std::mem::replace(&mut conn.body_handler, BodyHandler::None);
                
                match handler {
                    BodyHandler::File { ref mut file, ref path } => {
                        // File is closed when dropped or we can sync it.
                        file.sync_all().ok();
                        let filename = path.file_name().unwrap().to_string_lossy().to_string();
                        let response_body = "File uploaded successfully (streamed)";
                        let response = format!(
                            "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nLocation: {}\r\nConnection: keep-alive\r\n\r\n{}",
                            response_body.len(),
                            filename,
                            response_body
                        );
                        conn.write_buffer.extend_from_slice(response.as_bytes());
                    }
                    BodyHandler::Multipart { ref uploaded_files, upload_count, .. } => {
                        let response_body = format!("Uploaded {} files successfully:\n{}", upload_count, uploaded_files.join("\n"));
                        let response = format!(
                            "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        conn.write_buffer.extend_from_slice(response.as_bytes());
                    }
                    BodyHandler::Buffer(ref body_data) => {
                         // Fallback for buffered non-multipart POST (if any case hits this)
                            let content_type = conn
                                .request
                                .headers
                                .get("content-type")
                                .map(|s| s.as_str())
                                .unwrap_or("application/octet-stream");
                                
                            let extension = Self::get_ext_from_content_type(content_type);
                            let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
                            let filename = format!("upload_{}{}", timestamp, extension);
                            let mut upload_path = std::path::PathBuf::from(&route.root);
                            upload_path.push(&filename);
                            
                            if let Some(p) = upload_path.parent() { std::fs::create_dir_all(p).ok(); }
                            
                            if let Ok(mut f) = File::create(&upload_path) {
                                f.write_all(body_data).ok();
                                let response_body = "File uploaded successfully (buffered)";
                                let response = format!(
                                    "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nLocation: {}\r\nConnection: keep-alive\r\n\r\n{}",
                                    response_body.len(),
                                    filename,
                                    response_body
                                );
                                conn.write_buffer.extend_from_slice(response.as_bytes());
                            } else {
                                Self::queue_error(conn, registry, token, 500);
                            }
                    }
                    BodyHandler::None => {
                        // POST with no body?
                        Self::queue_error(conn, registry, token, 400); 
                    }
                }
            }
            "DELETE" => Self::handle_delete(conn, registry, token, route, &path)?,
            _ => {
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
