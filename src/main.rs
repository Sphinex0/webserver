use std::collections::HashMap;

use server::error::Result;

#[derive(Debug, PartialEq)]
pub enum ParsingState {
    RequestLine,
    Headers,
    Body(usize), // Content-Length
    Complete,
    Error,
}

pub struct HttpRequest {
    pub state: ParsingState,
    pub methode: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    buffer: Vec<u8>,
}

impl HttpRequest {
    pub fn new() -> Self {
        HttpRequest {
            state: ParsingState::RequestLine,
            methode: String::new(),
            path: String::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            buffer: Vec::new(),    
        }
    }

    pub fn append_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    pub fn parse_request_line(&mut self) -> std::result::Result<(), &'static str> {
        
        if let Some(crlf_pos) = find_crlf(&self.buffer) {
            println!("paaaaaaaaaaaaaaaaaaaaaaaaaaarsssssssssssssssssse {crlf_pos}");
            let line_bytes = self.buffer.drain(..crlf_pos + 2).collect();
            let line = match String::from_utf8(line_bytes) {
                Ok(line) => line.trim_end_matches("\r\n").to_string(),
                Err(_) => return Err("invalid utf-8 in request line"),
            };

            let parts = line.splitn(3, ' ').collect::<Vec<&str>>();
            if parts.len() != 3 {
                return Err("request line malformed");
            }

            self.methode = parts[0].to_string();
            self.path = parts[1].to_string();

            let version = parts[2];
            if version != "HTTP/1.1" {
                return Err("http version not supported use HTTP/1.1");
            }
            println!(
                "parsed request line: {} {} {}",
                self.methode, self.path, version
            );

            self.state = ParsingState::Headers;

            Ok(())
        } else {
            Err("Incomplete request line")
        }
    }

    pub fn parse(&mut self) -> std::result::Result<&ParsingState, &'static str> {
        loop {
            match self.state {
                ParsingState::RequestLine => {
                    if let Err(err) = self.parse_request_line() {
                        if err.contains("Incomplete") {
                            return Ok(&self.state);
                        }

                        self.state = ParsingState::Error;
                        return Err(err);
                    }
                }
                ParsingState::Headers => self.state = ParsingState::Complete,
                ParsingState::Body(_) => self.state = ParsingState::Complete,
                ParsingState::Complete | ParsingState::Error => return Ok(&self.state),
            }
        }
    }
}

// \r\n finder
fn find_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|window| window == b"\r\n")
}

fn main() -> Result<()> {
    let http = "\
GET /hello.htm HTTP/1.1\r\n\
Host: www.tutorialspoint.com\r\n\
User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36\r\n\
Accept-Language: en-us\r\n\
Connection: Keep-Alive\r\n\
\r\n\
";

    let http2 = "POST /cgi-bin/process.cgi HTTP/1.1
Host: www.tutorialspoint.com
Content-Type: application/x-www-form-urlencoded
Content-Length: 45

licenseID=string&content=string&paramsXML=string";

    let mut httpRequest = HttpRequest::new();
    let c = http.as_bytes();
    println!("{c:?}");
    httpRequest.buffer.extend_from_slice(c);
    httpRequest.parse()?;

    Ok(())
}
