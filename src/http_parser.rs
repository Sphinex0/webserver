use core::fmt;
use std::{collections::HashMap, fmt::Display};

#[derive(Debug, PartialEq, Clone)]
pub enum ParsingState {
    RequestLine,
    Headers,
    Body { remaining: usize },
    ChunkSize,
    ChunkBody { remaining: usize },
    Complete,
    Error,
}

pub struct HttpRequest {
    pub state: ParsingState,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub buffer: Vec<u8>,
}

impl HttpRequest {
    pub fn new() -> Self {
        HttpRequest {
            state: ParsingState::RequestLine,
            method: String::new(),
            path: String::new(),
            headers: HashMap::new(),
            buffer: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.state = ParsingState::RequestLine;
        self.headers.clear();
        // Buffer clearing is handled by consumption
    }

    pub fn append_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    pub fn parse_request_line(&mut self) -> Result<(), &'static str> {
        if let Some(crlf_pos) = find_crlf(&self.buffer) {
            // println!("parse_request_line");
            let line_bytes = self.buffer.drain(..crlf_pos + 2).collect();
            let line = match String::from_utf8(line_bytes) {
                Ok(line) => line.trim_end_matches("\r\n").to_string(),
                Err(_) => return Err("invalid utf-8 in request line"),
            };

            let parts = line.splitn(3, ' ').collect::<Vec<&str>>();
            if parts.len() != 3 {
                return Err("request line malformed");
            }

            self.method = parts[0].to_string();
            self.path = parts[1].to_string();

            let version = parts[2];
            if version != "HTTP/1.1" {
                return Err("http version not supported use HTTP/1.1");
            }
            // println!(
            //     "parsed request line: {} {} {}",
            //     self.method, self.path, version
            // );

            self.state = ParsingState::Headers;

            Ok(())
        } else {
            println!("notfound crlf");
            Err("Incomplete request line")
        }
    }

    fn parse_headers(&mut self) -> Result<(), &'static str> {
        loop {
            match extract_and_parse_header_line(&mut self.buffer)? {
                Some((key, value)) => {
                    if key == "Incomplete" {
                        // Header line incomplete. Check if buffer is already too large.
                        if self.buffer.len() > 8 * 1024 {
                            return Err("header passed the maximum size");
                        }
                        return Err("Incomplete");
                    }
                    self.headers.insert(key.to_lowercase(), value);
                }
                None => {
                    let is_chunked = self.headers.get("transfer-encoding")
                        .map(|v| v.to_lowercase().contains("chunked"))
                        .unwrap_or(false);

                    if is_chunked {
                        self.state = ParsingState::ChunkSize;
                    } else {
                        let content_length = self.headers.get("content-length")
                            .and_then(|val| val.parse::<usize>().ok())
                            .unwrap_or(0);

                        if content_length > 0 {
                            self.state = ParsingState::Body { remaining: content_length };
                        } else {
                            self.state = ParsingState::Complete;
                        }
                    }
                    return Ok(());
                }
            }
        }
    }

    pub fn consume_body(&mut self, count: usize) {
        if count > self.buffer.len() {
            self.buffer.clear();
        } else {
            self.buffer.drain(..count);
        }
        
        match self.state {
            ParsingState::Body { remaining } => {
                let new_remaining = remaining.saturating_sub(count);
                self.state = ParsingState::Body { remaining: new_remaining };
            }
            ParsingState::ChunkBody { remaining } => {
                let new_remaining = remaining.saturating_sub(count);
                self.state = ParsingState::ChunkBody { remaining: new_remaining };
            }
            _ => {}
        }
    }

    pub fn parse(&mut self) -> Result<ParsingState, &'static str> {
        loop {
            match self.state {
                ParsingState::RequestLine => {
                    if let Err(err) = self.parse_request_line() {
                        if err.contains("Incomplete") {
                            return Ok(self.state.clone());
                        }
                        self.state = ParsingState::Error;
                        return Err(err);
                    }
                }
                ParsingState::Headers => {
                    if let Err(err) = self.parse_headers() {
                        if err.contains("Incomplete") {
                            return Ok(self.state.clone());
                        }
                        self.state = ParsingState::Error;
                        return Err(err);
                    }
                }
                ParsingState::Body { remaining } => {
                    if remaining == 0 {
                        self.state = ParsingState::Complete;
                        return Ok(self.state.clone());
                    }
                    return Ok(self.state.clone());
                },
                ParsingState::ChunkSize => {
                    if let Some(crlf_pos) = find_crlf(&self.buffer) {
                        let line_bytes: Vec<u8> = self.buffer.drain(..crlf_pos + 2).collect();
                        let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                        let size_str = line.split(';').next().unwrap_or("").trim();
                        
                        match usize::from_str_radix(size_str, 16) {
                            Ok(size) => {
                                if size == 0 {
                                    if self.buffer.starts_with(b"\r\n") {
                                        self.buffer.drain(..2);
                                    }
                                    self.state = ParsingState::Complete;
                                } else {
                                    self.state = ParsingState::ChunkBody { remaining: size };
                                }
                            }
                            Err(_) => {
                                self.state = ParsingState::Error;
                                return Err("Invalid chunk size");
                            }
                        }
                    } else {
                        return Ok(self.state.clone());
                    }
                },
                ParsingState::ChunkBody { remaining } => {
                    if remaining == 0 {
                        if self.buffer.len() >= 2 {
                            if &self.buffer[0..2] == b"\r\n" {
                                self.buffer.drain(..2);
                                self.state = ParsingState::ChunkSize;
                            } else {
                                self.state = ParsingState::Error;
                                return Err("Invalid chunk format");
                            }
                        } else {
                            return Ok(self.state.clone());
                        }
                    } else {
                        return Ok(self.state.clone());
                    }
                },
                ParsingState::Complete | ParsingState::Error => return Ok(self.state.clone()),
            }
        }
    }
}
/* HELPER FUNCTIONS */
// \r\n finder
fn find_crlf(buffer: &[u8]) -> Option<usize> {

    let mut current_pos = 0;
    while let Some(r_pos) = buffer[current_pos..].iter().position(|&b| b == b'\r') {
        
        let abs_r_pos_in_search = current_pos + r_pos;

        if buffer.get(abs_r_pos_in_search + 1) == Some(&b'\n') {
            // Return the absolute position in the original 'buffer'
            return Some(abs_r_pos_in_search);
        }
        current_pos = abs_r_pos_in_search + 1;
    }
    None
}

fn extract_and_parse_header_line(
    buffer: &mut Vec<u8>,
) -> Result<Option<(String, String)>, &'static str> {
    if let Some(crlf_pos) = find_crlf(buffer) {
        //end of header
        if crlf_pos == 0 {
            buffer.drain(..2);
            return Ok(None);
        }

        let line_bytes = buffer.drain(..crlf_pos + 2).collect();
        let line = match String::from_utf8(line_bytes) {
            Ok(line) => line.trim_end_matches("\r\n").to_string(),
            Err(_) => return Err("invalid utf-8 in request line"),
        };

        if let Some(ddot_pos) = line.find(":") {
            let key = line[..ddot_pos].trim().to_string();
            let value = line[ddot_pos + 1..].trim().to_string();

            return Ok(Some((key, value)));
        } else {
            return Err("Malformed header");
        }
    } else {
        Ok(Some(("Incomplete".to_string(), "".to_string())))
    }
}

impl Display for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "--- HTTP Request ---")?;
        // 1. Request Line: GET /path HTTP/1.1
        writeln!(f, "{:?} {} HTTP/1.1", self.method, self.path)?;

        // 2. Headers: Key: Value
        writeln!(f, "Headers:")?;
        for (key, value) in &self.headers {
            writeln!(f, "  {}: {}", key, value)?;
        }

        // 3. Body Summary
        // Body is streamed, so we can't print it easily from here without consuming buffer.
        // We'll just print buffer size.
        writeln!(f, "Buffer ({} bytes)", self.buffer.len())?;
        writeln!(f, "--------------------")
    }
}