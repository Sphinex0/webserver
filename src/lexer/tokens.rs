#[derive(Debug, PartialEq, Clone)]
pub enum TokenType {
    Text(String),        // host, 127.0.0.1
    StringLit(String),   // "host", "GET"
    Number(u64),         // 8080
    Colon,               // :
    Dash,                // -
    LBracket,            // [
    RBracket,            // ]
    Comma,               // ,
    Newline,             // \n
    Indent(usize),       // Critical for location blocks
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenType,
    pub loc: Loc,
}

#[derive(Debug, Clone, Copy)]
pub struct Loc {
    pub line: usize,
    pub col: usize,
}