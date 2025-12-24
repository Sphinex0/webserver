use std::collections::HashMap;

use crate::httpParser::HttpRequest;

type Handler = fn(&HttpRequest) -> String;

pub struct Router {
    routes: HashMap<String, Handler>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    pub fn add_route(&mut self, path: &str, handler: Handler) {
        self.routes.insert(path.to_string(), handler);
    }

    pub fn handle(&self, request: &HttpRequest) -> String {
        // println!("{}",request.path);
        match self.routes.get(&request.path) {
            Some(handler) => handler(request),
            None => "HTTP/1.1 404 NOT FOUND\r\nContent-Length: 9\r\n\r\nNot Found".to_string()
        }
    }
}
