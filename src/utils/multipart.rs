use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum MultipartEvent {
    StartPart(HashMap<String, String>),
    PartData(Vec<u8>),
    EndPart,
    Finished,
    Error(String),
}

enum ParserState {
    Preamble,
    Headers,
    Body,
    Epilogue,
}

pub struct MultipartParser {
    boundary: String,
    state: ParserState,
    buffer: Vec<u8>,
    delimiter: Vec<u8>, // "--boundary"
    closing_delimiter: Vec<u8>, // "--boundary--"
}

impl MultipartParser {
    pub fn new(boundary: &str) -> Self {
        Self {
            boundary: boundary.to_string(),
            state: ParserState::Preamble,
            buffer: Vec::new(),
            delimiter: format!("--{}", boundary).into_bytes(),
            closing_delimiter: format!("--{}--", boundary).into_bytes(),
        }
    }

    pub fn process(&mut self, chunk: &[u8]) -> Vec<MultipartEvent> {
        let mut events = Vec::new();
        self.buffer.extend_from_slice(chunk);

        loop {
            match self.state {
                ParserState::Preamble => {
                    if let Some(pos) = self.find_delimiter(&self.delimiter) {
                        self.buffer.drain(..pos + self.delimiter.len());
                        self.state = ParserState::Headers;
                    } else {
                        let keep = self.delimiter.len();
                        if self.buffer.len() > keep {
                            self.buffer.drain(..self.buffer.len() - keep);
                        }
                        break;
                    }
                }
                ParserState::Headers => {
                    if let Some(pos) = self.find_double_crlf() {
                        let header_bytes: Vec<u8> = self.buffer.drain(..pos).collect();
                        self.buffer.drain(..4); // Consume \r\n\r\n

                        let headers = parse_headers(&header_bytes);
                        events.push(MultipartEvent::StartPart(headers));
                        self.state = ParserState::Body;
                    } else {
                        if self.buffer.len() > 16 * 1024 {
                             events.push(MultipartEvent::Error("Headers too large".to_string()));
                             return events;
                        }
                        break;
                    }
                }
                ParserState::Body => {
                    let full_delim = [&b"\r\n"[..], &self.delimiter].concat();
                    
                    if let Some(pos) = self.find_delimiter(&full_delim) {
                        let data: Vec<u8> = self.buffer.drain(..pos).collect();
                        if !data.is_empty() {
                            events.push(MultipartEvent::PartData(data));
                        }
                        events.push(MultipartEvent::EndPart);
                        
                        self.buffer.drain(..full_delim.len());
                        
                        self.state = ParserState::Headers; // Assume next part, will check for --
                        
                        if self.buffer.starts_with(b"--") {
                             self.buffer.drain(..2);
                             self.state = ParserState::Epilogue;
                             events.push(MultipartEvent::Finished);
                        } else if self.buffer.starts_with(b"\r\n") {
                             self.buffer.drain(..2);
                        }
                    } else {
                        let safe_len = full_delim.len() + 2;
                        if self.buffer.len() > safe_len {
                            let drain_len = self.buffer.len() - safe_len;
                            let data: Vec<u8> = self.buffer.drain(..drain_len).collect();
                            events.push(MultipartEvent::PartData(data));
                        }
                        break;
                    }
                }
                ParserState::Epilogue => {
                    self.buffer.clear();
                    break;
                }
            }
        }
        events
    }

    pub fn finalize(&mut self) -> Vec<MultipartEvent> {
        let mut events = Vec::new();
        if matches!(self.state, ParserState::Body) {
             // We reached EOF but didn't find the closing boundary?
             // Or maybe it's in the buffer but we were holding it for safety.
             // Try one last search for the closing boundary without \r\n prefix (could be a malformed/truncated stream)
             // or just flush.
             if !self.buffer.is_empty() {
                  events.push(MultipartEvent::PartData(self.buffer.drain(..).collect()));
             }
             events.push(MultipartEvent::EndPart);
             events.push(MultipartEvent::Finished);
        }
        events
    }

    fn find_delimiter(&self, delim: &[u8]) -> Option<usize> {
        self.buffer.windows(delim.len()).position(|window| window == delim)
    }

    fn find_double_crlf(&self) -> Option<usize> {
        self.buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}

fn parse_headers(buffer: &[u8]) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Ok(header_str) = std::str::from_utf8(buffer) {
        for line in header_str.lines() {
            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim().to_string();
                let value = line[idx+1..].trim().to_string();
                headers.insert(key, value); 
            }
        }
    }
    headers
}

