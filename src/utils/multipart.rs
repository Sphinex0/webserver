use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub enum MultipartEvent {
    StartPart(HashMap<String, String>),
    PartData(Vec<u8>),
    EndPart,
    Finished,
    Error(String),
}

/// Lightweight event type for zero-copy processing. `PartData` gives a slice
/// borrowed from the internal buffer. The sink *must* consume or copy the
/// slice before returning; the slice becomes invalid if the parser trims the
/// buffer.
pub enum MultipartEventRef<'a> {
    StartPart(HashMap<String, String>),
    PartData(&'a [u8]),
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
    consumed: usize,
    delimiter: Vec<u8>, // "--boundary"
    closing_delimiter: Vec<u8>, // "--boundary--"
}

impl MultipartParser {
    pub fn new(boundary: &str) -> Self {
        Self {
            boundary: boundary.to_string(),
            state: ParserState::Preamble,
            buffer: Vec::new(),
            consumed: 0,
            delimiter: format!("--{}", boundary).into_bytes(),
            closing_delimiter: format!("--{}--", boundary).into_bytes(),
        }
    }

    /// Zero-copy processing that sends events to `sink`. The sink receives
    /// borrowed slices for `PartData` which must be used synchronously (written
    /// or copied) before returning. If the sink returns `false` the parser will
    /// stop processing further events for this chunk.
    pub fn process_to_sink<F>(&mut self, chunk: &[u8], mut sink: F)
    where
        F: FnMut(MultipartEventRef) -> bool,
    {
        self.buffer.extend_from_slice(chunk);

        loop {
            match self.state {
                ParserState::Preamble => {
                    if let Some(pos) = self.find_delimiter(&self.delimiter) {
                        // advance consumed past delimiter
                        self.consumed = pos + self.delimiter.len();
                        self.state = ParserState::Headers;
                    } else {
                        let keep = self.delimiter.len();
                        if self.buffer.len().saturating_sub(self.consumed) > keep {
                            // trim prefix but keep `keep` bytes for partial match
                            let remove = self.buffer.len().saturating_sub(self.consumed) - keep;
                            self.buffer.drain(..remove);
                            // adjust consumed
                            if self.consumed >= remove {
                                self.consumed -= remove;
                            } else {
                                self.consumed = 0;
                            }
                        }
                        break;
                    }
                }
                ParserState::Headers => {
                    if let Some(pos) = self.find_double_crlf() {
                        // header bytes are from consumed .. pos
                        let header_slice = &self.buffer[self.consumed..pos];
                        self.consumed = pos + 4; // consume headers + \r\n\r\n

                        let headers = parse_headers(header_slice);
                        if !sink(MultipartEventRef::StartPart(headers)) { return; }
                        self.state = ParserState::Body;
                    } else {
                        if self.buffer.len().saturating_sub(self.consumed) > 16 * 1024 {
                             let _ = sink(MultipartEventRef::Error("Headers too large".to_string()));
                             return;
                        }
                        break;
                    }
                }
                ParserState::Body => {
                    let mut full_delim = Vec::with_capacity(2 + self.delimiter.len());
                    full_delim.extend_from_slice(b"\r\n");
                    full_delim.extend_from_slice(&self.delimiter);

                    if let Some(pos) = self.find_delimiter(&full_delim) {
                        let data_slice = &self.buffer[self.consumed..pos];
                        if !data_slice.is_empty() {
                            if !sink(MultipartEventRef::PartData(data_slice)) { return; }
                        }
                        if !sink(MultipartEventRef::EndPart) { return; }

                        self.consumed = pos + full_delim.len();

                        // Check for closing markers
                        if self.buffer.get(self.consumed..).map(|s| s.starts_with(b"--")).unwrap_or(false) {
                             self.consumed += 2;
                             let _ = sink(MultipartEventRef::Finished);
                             self.state = ParserState::Epilogue;
                        } else if self.buffer.get(self.consumed..).map(|s| s.starts_with(b"\r\n")).unwrap_or(false) {
                             self.consumed += 2;
                             self.state = ParserState::Headers;
                        } else {
                            self.state = ParserState::Headers;
                        }
                    } else {
                        let safe_len = full_delim.len() + 2;
                        let avail = self.buffer.len().saturating_sub(self.consumed);
                        if avail > safe_len {
                            let drain_len = avail - safe_len;
                            let data_slice = &self.buffer[self.consumed..self.consumed + drain_len];
                            if !data_slice.is_empty() {
                                if !sink(MultipartEventRef::PartData(data_slice)) { return; }
                            }
                            self.consumed += drain_len;
                        }
                        break;
                    }
                }
                ParserState::Epilogue => {
                    self.buffer.clear();
                    self.consumed = 0;
                    break;
                }
            }
        }

        // If consumed grows large, drop consumed prefix to keep buffer bounded
        if self.consumed > 64 * 1024 {
            self.buffer.drain(..self.consumed);
            self.consumed = 0;
        }
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
        let buf = &self.buffer[self.consumed..];
        if delim.is_empty() || buf.len() < delim.len() {
            return None;
        }

        let first_byte = delim[0];
        let mut search_start = 0;

        while let Some(pos) = buf[search_start..].iter().position(|&b| b == first_byte) {
            let absolute_pos = search_start + pos;
            if absolute_pos + delim.len() > buf.len() {
                return None;
            }
            if &buf[absolute_pos..absolute_pos + delim.len()] == delim {
                return Some(self.consumed + absolute_pos);
            }
            search_start = absolute_pos + 1;
        }
        None
    }

    fn find_double_crlf(&self) -> Option<usize> {
        let buf = &self.buffer[self.consumed..];
        buf.windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|p| self.consumed + p)
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

