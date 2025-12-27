// pub enum Token {
//     EOF,
//     KEY(String),
//     VALUE(String),
//     COLON,
//     DASH,
//     OPENBRACKET,
//     CLOSEBRACKET,
//     COMA,
//     Whitespace,
// }

// pub struct Token {
//     pub kind: TokenKind,
//     pub value: String,
// }

// impl Token {
//     pub fn new(kind: TokenKind, value: String) -> Token {
//         Token { kind, value }
//     }
// }

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Server,         // "server"
    Location,       // "location"
    Identifier(String), // e.g., "root", "listen", "127.0.0.1"
    Colon,          // ":"
    Indent(usize),  // Number of leading spaces
    Newline,
    EOF,
}
