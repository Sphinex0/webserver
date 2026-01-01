extern crate proc_macro;
use proc_macro::{TokenStream, TokenTree, Delimiter};

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
                            if s != "pub" && s != "ConfigParser" && s != "ParseResult" && s != "FromYaml" {
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
                                         if p.as_char() == ',' { break; }
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

    let q = char::from(34);
    let mut arms = String::new();
    for field in fields {
        let arm = format!(
            "{0}{1}{0} => {{ parser.consume_key({0}{1}{0})?; obj.{1} = FromYaml::from_yaml(parser, struct_indent.unwrap_or(min_indent)).map_err(|mut e| {{ e.context.push(format!({0}parsing field '{1}'{0})); e }})?; }},
",
            q, field
        );
        arms.push_str(&arm);
    }

    let mut code = "impl FromYaml for STRUCT {
    fn from_yaml(parser: &mut ConfigParser, min_indent: usize) -> ParseResult<Self> {
        let mut obj = Self::default();
        let mut struct_indent: Option<usize> = None;
        loop {
            parser.skip_newlines_only();
            
            if let Some(crate::lexer::tokens::TokenType::Indent(n)) = parser.peek_kind() {
                let indent = *n;
                if indent < min_indent { break; }
                
                if let Some(crate::lexer::tokens::TokenType::Dash) = parser.peek_kind_at(1) {
                    break;
                }

                if let Some(current) = struct_indent {
                    if indent != current {
                        if indent < current {
                            if indent > min_indent {
                                return Err(ConfigError {
                                    message: format!(\"Indentation mismatch: found {} < current {} but > parent {}\", indent, current, min_indent),
                                    loc: parser.peek_loc(),
                                    context: vec![],
                                });
                            }
                            break;
                        } else {
                             return Err(ConfigError {
                                 message: format!(\"Indentation mismatch: found {} > current {}\", indent, current),
                                 loc: parser.peek_loc(),
                                 context: vec![],
                             });
                        }
                    }
                } else {
                    if indent <= min_indent && min_indent > 0 { break; }
                    struct_indent = Some(indent);
                }
                parser.cursor += 1; 
            } else if min_indent > 0 {
                if struct_indent.is_none() && min_indent > 0 {
                }
            }

            if let Some(crate::lexer::tokens::TokenType::Dash) = parser.peek_kind() { break; }

            let key_str = match parser.peek_kind() {
                Some(crate::lexer::tokens::TokenType::Text(s)) | Some(crate::lexer::tokens::TokenType::StringLit(s)) => {
                    if let Some(crate::lexer::tokens::TokenType::Colon) = parser.peek_kind_at(1) {
                        s.clone()
                    } else {
                        return Err(ConfigError {
                            message: format!(\"Expected key-value pair, found scalar '{}'\", s),
                            loc: parser.peek_loc(),
                            context: vec![],
                        });
                    }
                },
                Some(crate::lexer::tokens::TokenType::Number(n)) => {
                    return Err(ConfigError {
                        message: format!(\"Expected map key, found number '{}'\", n),
                        loc: parser.peek_loc(),
                        context: vec![],
                    });
                }
                Some(t) => {
                     return Err(ConfigError {
                        message: format!(\"Expected map key, found {:?}\", t),
                        loc: parser.peek_loc(),
                        context: vec![],
                    });
                }
                None => break,
            };

            match key_str.as_str() {
                ARMS
                _ => {
                    eprintln!(\"Warning: Unknown field '{}'\", key_str);
                    parser.consume_key(&key_str)?;
                    // Pass struct_indent if available, else min_indent.
                    // Actually, for unknown fields, we want to skip based on *their* indent?
                    // skip_value handles indentation.
                    parser.skip_value(struct_indent.unwrap_or(min_indent))?;
                }
            }
        }
        Ok(obj)
    }
}".to_string();

    code = code.replace("STRUCT", &struct_name);
    code = code.replace("ARMS", &arms);

    code.parse().expect("Generated code was invalid")
}
