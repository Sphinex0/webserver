use std::collections::HashMap;

#[derive(Debug)]
pub struct MultipartPart {
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub filename: Option<String>,
    pub name: Option<String>,
    pub content_type: Option<String>,
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Vec<MultipartPart> {
    let mut parts = Vec::new();
    let delimiter = format!("--{}", boundary);
    let delimiter_bytes = delimiter.as_bytes();
    
    // Naive split by delimiter
    // Note: This assumes the boundary doesn't appear in the binary data. 
    // A robust parser would use a state machine or the Knuth-Morris-Pratt algorithm.
    // For this prototype, we'll use a simple window search or split logic.
    
    // We can't use split directly on &[u8] with a pattern easily without a crate like `bstr` or manual impl.
    // Let's implement a manual split.
    
    let mut positions = Vec::new();
    let mut i = 0;
    while i <= body.len().saturating_sub(delimiter_bytes.len()) {
        if &body[i..i+delimiter_bytes.len()] == delimiter_bytes {
            positions.push(i);
            i += delimiter_bytes.len();
        } else {
            i += 1;
        }
    }
    
    if positions.is_empty() {
        return parts;
    }

    // Iterate over intervals defined by positions
    for i in 0..positions.len() {
        let start = positions[i] + delimiter_bytes.len();
        let end = if i + 1 < positions.len() {
            positions[i+1]
        } else {
            body.len()
        };
        
        let chunk = &body[start..end];
        
        // chunk usually starts with optional \r\n (except first one maybe?) or -- (if end)
        // and ends with \r\n (before the next boundary).
        
        // If it starts with --, it's the end.
        if chunk.starts_with(b"--") {
            break;
        }
        
        // Trim leading CRLF if present
        let mut slice = chunk;
        if slice.starts_with(b"\r\n") {
            slice = &slice[2..];
        } else if slice.starts_with(b"\n") {
             slice = &slice[1..];
        }
        
        // Trim trailing CRLF if present (it belongs to the boundary separation)
        if slice.ends_with(b"\r\n") {
            slice = &slice[..slice.len()-2];
        } else if slice.ends_with(b"\n") {
            slice = &slice[..slice.len()-1];
        }
        
        if slice.is_empty() { continue; }

        // Find double CRLF separating headers from body
        let header_end_pos = find_double_crlf(slice);
        
        if let Some(pos) = header_end_pos {
            let header_bytes = &slice[..pos];
            // Body starts after \r\n\r\n (pos + 4)
            let body_bytes = &slice[pos+4..];
            
            let headers = parse_headers(header_bytes);
            
            let mut part = MultipartPart {
                headers: headers.clone(),
                body: body_bytes.to_vec(),
                filename: None,
                name: None,
                content_type: headers.get("Content-Type").cloned(),
            };
            
            // Parse Content-Disposition
            if let Some(disp) = headers.get("Content-Disposition") {
                // Example: form-data; name="field2"; filename="example.txt"
                let parts: Vec<&str> = disp.split(';').collect();
                for p in parts {
                    let p = p.trim();
                    if p.starts_with("filename=") {
                        let val = p.trim_start_matches("filename=").trim_matches('"');
                        part.filename = Some(val.to_string());
                    } else if p.starts_with("name=") {
                        let val = p.trim_start_matches("name=").trim_matches('"');
                        part.name = Some(val.to_string());
                    }
                }
            }
            
            parts.push(part);
        }
    }
    dbg!(&parts);
    parts
}

fn find_double_crlf(buffer: &[u8]) -> Option<usize> {
    for i in 0..buffer.len().saturating_sub(4) {
        if &buffer[i..i+4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

fn parse_headers(buffer: &[u8]) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    if let Ok(header_str) = std::str::from_utf8(buffer) {
        for line in header_str.lines() {
            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim().to_string();
                let value = line[idx+1..].trim().to_string();
                headers.insert(key, value); // Case sensitivity? usually headers are case insensitive
            }
        }
    }
    headers
}
