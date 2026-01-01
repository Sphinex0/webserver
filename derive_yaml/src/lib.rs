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
        let mut flags = String::new();
        let mut arms = String::new();
        
            for field in &fields {
        
                flags.push_str(&format!("let mut seen_{} = false;\n", field));
        
                
        
                let arm = format!(
        
                    "{0}{1}{0} => {{ 
        
                        if seen_{1} {{
        
                            return Err(crate::config::ConfigError {{
        
                                message: format!({0}Duplicate field '{1}'{0}),
        
                                loc: parser.peek_loc(),
        
                                context: vec![]
        
                            }});
        
                        }}
        
                        seen_{1} = true;
        
                        parser.consume_key(&key)?; 
        
                        obj.{1} = FromYaml::from_yaml(parser, min_indent)
        
                            .map_err(|mut e| {{ e.context.push(format!({0}parsing field '{1}'{0})); e }})?; 
        
                    }},
        
        ",
        
                    q, field
        
                );
        
                arms.push_str(&arm);
        
            }
        
        
        
            let mut code = "impl FromYaml for STRUCT {
        
            fn from_yaml(parser: &mut crate::config::ConfigParser, min_indent: usize) -> crate::config::ParseResult<Self> {
        
                let mut obj = Self::default();
        
                let mut struct_indent: Option<usize> = None;
        
                FLAGS
        
                loop {            if !parser.check_indentation(min_indent, &mut struct_indent)? {
                break;
            }
            if parser.is_end_of_block() {
                break;
            }
            let key = match parser.parse_map_key()? {
                Some(k) => k,
                None => break,
            };

            match key.as_str() {
                ARMS
                _ => {
                    eprintln!(\"Warning: Unknown field '{}'\", key);
                    parser.consume_key(&key)?;
                    parser.skip_value(struct_indent.unwrap_or(min_indent))?;
                }
            }
        }
        Ok(obj)
    }
}".to_string();

    code = code.replace("STRUCT", &struct_name);
    code = code.replace("FLAGS", &flags);
    code = code.replace("ARMS", &arms);

    code.parse().expect("Generated code was invalid")
}