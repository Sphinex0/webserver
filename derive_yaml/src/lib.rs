use proc_macro::{Delimiter, TokenStream, TokenTree};

#[proc_macro_derive(FromYaml)]
pub fn derive_from_yaml(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter();
    let mut struct_name = String::new();
    let mut fields = Vec::new();

    while let Some(token) = tokens.next() {
        if let TokenTree::Ident(ident) = &token {
            if ident.to_string() == "struct" {
                if let Some(TokenTree::Ident(name)) = tokens.next() {
                    struct_name = name.to_string();
                    break;
                }
            }
        }
    }

    while let Some(token) = tokens.next() {
        if let TokenTree::Group(group) = token {
            if group.delimiter() == Delimiter::Brace {
                let mut group_iter = group.stream().into_iter();
                let mut last_ident = String::new();
                while let Some(inner_token) = group_iter.next() {
                    match inner_token {
                        TokenTree::Ident(ident) => {
                            let s = ident.to_string();
                            if s != "pub"
                                && s != "ConfigParser"
                                && s != "ParseResult"
                                && s != "FromYaml"
                            {
                                last_ident = s;
                            }
                        }
                        TokenTree::Punct(punct) => {
                            if punct.as_char() == ':' {
                                if !last_ident.is_empty() {
                                    fields.push(last_ident.clone());
                                    last_ident.clear();
                                }
                                while let Some(skip_token) = group_iter.next() {
                                    if let TokenTree::Punct(p) = &skip_token {
                                        if p.as_char() == ',' {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                break;
            }
        }
    }

    let q = char::from(34); // '"'
    let mut arms = String::new();
    dbg!(&q);
    for field in fields {
        let arm = format!("{0}{1}{0} => {{ parser.consume_key({0}{1}{0})?; obj.{1} = FromYaml::from_yaml(parser, min_indent)?; }},
", q, field);
        arms.push_str(&arm);
    }

    let mut code = "impl FromYaml for STRUCT {
        fn from_yaml(parser: &mut ConfigParser, min_indent: usize) -> ParseResult<Self> {
            let mut obj = Self::default();
            loop {
                let has_newline = parser.skip_newlines_only();
                if let Some(crate::lexer::tokens::TokenType::Indent(n)) = parser.peek_kind() {
                    if *n < min_indent || (*n == min_indent && min_indent > 0) { break; }
                    parser.tokens.next();
                } else if has_newline && min_indent > 0 {
                    break;
                }
                if let Some(crate::lexer::tokens::TokenType::Dash) = parser.peek_kind() { break; }
                let key_str = match parser.peek_kind() {
                    Some(crate::lexer::tokens::TokenType::Text(s)) | Some(crate::lexer::tokens::TokenType::StringLit(s)) => s.clone(),
                    _ => break,
                };
                match key_str.as_str() {
                    ARMS
                    _ => {break;},
                }
            }
            Ok(obj)
        }
    }".to_string();

    code = code.replace("STRUCT", &struct_name);
    code = code.replace("ARMS", &arms);

    code.parse().expect("Generated code was invalid")
}
