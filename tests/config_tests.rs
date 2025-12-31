use server::config::{Config, FromYaml};

#[test]
fn test_valid_config() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080, 8081]
    server_name: "test_server"
    default_server: true
    client_max_body_size: 1024
    routes:
      - path: "/"
        methods: ["GET"]
        root: "./www"
        default_file: "index.html"
        autoindex: true
"#;
    let config = Config::from_str(yaml).expect("Should parse valid config");
    dbg!(&config.servers);
    assert_eq!(config.servers.len(), 1);
    let server = &config.servers[0];
    assert_eq!(server.host, "127.0.0.1");
    assert_eq!(server.ports, vec![8080, 8081]);
    assert_eq!(server.server_name, "test_server");
    assert!(server.default_server);
    assert_eq!(server.client_max_body_size, 1024);
    assert_eq!(server.routes.len(), 1);
    assert_eq!(server.routes[0].path, "/");
}

#[test]
fn test_missing_colon() {
    let yaml = r#"
servers:
  - host "127.0.0.1"
"#;
    let err = Config::from_str(yaml).unwrap_err();
    // Expect parsing to fail. The specific error message depends on where it fails.
    // It should expect a Colon but found StringLit probably.
    println!("{}", err);
    assert!(err.message.contains("Expected Colon") || err.message.contains("Expected"));
}

#[test]
fn test_wrong_indentation() {
    let yaml_bad = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
   server_name: "bad_indent"
"#;
    let err = Config::from_str(yaml_bad).unwrap_err();
    println!("{}", err);
    assert!(err.message.contains("Indentation mismatch"));
}

#[test]
fn test_unknown_field() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    unknown_field: "some_value"
    unknown_block:
      nested: "value"
      list: [1, 2]
    server_name: "test"
"#;
    // New behavior: unknown field is skipped with warning.
    // server_name should be parsed.
    let config = Config::from_str(yaml).expect("Parses successfully");
    assert_eq!(config.servers[0].host, "127.0.0.1");
    assert_eq!(config.servers[0].server_name, "test");
}

#[test]
fn test_boolean_values() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
    routes:
      - path: "/on"
        autoindex: on
      - path: "/true"
        autoindex: true
      - path: "/off"
        autoindex: off
      - path: "/false"
        autoindex: false
"#;
    let config = Config::from_str(yaml).expect("Failed to parse boolean values");
    let routes = &config.servers[0].routes;
    
    // Check 'on'
    assert!(routes.iter().find(|r| r.path == "/on").unwrap().autoindex);
    // Check 'true'
    assert!(routes.iter().find(|r| r.path == "/true").unwrap().autoindex);
    // Check 'off'
    assert!(!routes.iter().find(|r| r.path == "/off").unwrap().autoindex);
    // Check 'false'
    assert!(!routes.iter().find(|r| r.path == "/false").unwrap().autoindex);
}

#[test]
fn test_error_pages_map() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
    error_pages:
      404: "404.html"
      500: "500.html"
"#;
    let config = Config::from_str(yaml).expect("Failed to parse error pages");
    let error_pages = &config.servers[0].error_pages;
    
    assert_eq!(error_pages.get(&404).map(|s| s.as_str()), Some("404.html"));
    assert_eq!(error_pages.get(&500).map(|s| s.as_str()), Some("500.html"));
    assert_eq!(error_pages.len(), 2);
}

#[test]
fn test_multiple_servers() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
    server_name: "server1"
  - host: "0.0.0.0"
    ports: [9090]
    server_name: "server2"
"#;
    let config = Config::from_str(yaml).expect("Failed to parse multiple servers");
    assert_eq!(config.servers.len(), 2);
    assert_eq!(config.servers[0].server_name, "server1");
    assert_eq!(config.servers[1].server_name, "server2");
    assert_eq!(config.servers[1].host, "0.0.0.0");
}

#[test]
fn test_optional_fields_defaults() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
    routes:
      - path: "/"
"#;
    let config = Config::from_str(yaml).expect("Failed to parse minimal route");
    let route = &config.servers[0].routes[0];
    
    assert_eq!(route.redirection, None);
    assert_eq!(route.cgi_ext, None);
    assert_eq!(route.methods, vec!["GET", "HEAD"]); // Default
    assert!(!route.autoindex); // Default false
}

#[test]
fn test_route_matching_logic() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080]
    routes:
      - path: "/"
      - path: "/api"
      - path: "/api/v1"
"#;
    let config = Config::from_str(yaml).expect("Failed to parse for route match");
    let server = &config.servers[0];
    
    // Exact match
    let r1 = server.find_route("/").expect("Should find root");
    assert_eq!(r1.path, "/");
    
    // Prefix match
    let r2 = server.find_route("/api/users").expect("Should find api");
    assert_eq!(r2.path, "/api");
    
    // Longest prefix match
    let r3 = server.find_route("/api/v1/users").expect("Should find api v1");
    assert_eq!(r3.path, "/api/v1");
}

#[test]
fn test_dashed_list_indentation() {
    // Standard YAML allows dashes at the same level as the field or indented.
    // Our parser should handle consistent indentation for list items.
    let yaml = r#"
servers:
  - host: "s1"
    ports:
      - 80
      - 81
  - host: "s2"
    ports: [90, 91]
"#;
    let config = Config::from_str(yaml).expect("Failed to parse standard dashed list");
    assert_eq!(config.servers.len(), 2);
    assert_eq!(config.servers[0].ports, vec![80, 81]);
}

#[test]
fn test_dashed_list_deep_indentation() {
    let yaml = r#"
servers:
  - host: "s1"
    ports:
        - 80
        - 81
"#;
    let config = Config::from_str(yaml).expect("Failed to parse deep indented list");
    assert_eq!(config.servers[0].ports, vec![80, 81]);
}

#[test]
fn test_dashed_list_mismatched_indentation() {
    let yaml = r#"
servers:
  - host: "s1"
    ports:
      - 80
     - 81
"#;
    // With strict parsing, this should fail because '- 81' is indented at 5 spaces
    // while the 'ports' list started at 6 spaces (implied/normalized).
    // Or if strictness applies to the 'servers' list?
    // 'ports' starts at indent 6.
    // '- 81' is at indent 5.
    // Indent(5) < Indent(6) -> End of 'ports' list.
    // Then parser sees Indent(5) followed by Dash.
    // This is processed by 'servers' list parser (indent 2).
    // Indent(5) > Indent(2) AND followed by Dash -> Indentation mismatch error!
    
    let err = Config::from_str(yaml).unwrap_err();
    assert!(err.message.contains("Indentation mismatch"));
}

#[test]
fn test_separate_list_items_behavior() {
    let yaml = r#"
servers:
  - host: "127.0.0.255"
  - ports: [9999]
"#;
    let config = Config::from_str(yaml).expect("Should parse");
    assert_eq!(config.servers.len(), 2);
    
    // Server 1
    assert_eq!(config.servers[0].host, "127.0.0.255");
    assert_eq!(config.servers[0].ports, vec![8080]); // Default
    
    // Server 2
    assert_eq!(config.servers[1].host, "127.0.0.1"); // Default
    assert_eq!(config.servers[1].ports, vec![9999]);
}

#[test]
fn test_scalar_where_list_expected() {
    let yaml = r#"
servers:
  - ports: 9999
"#;
    // This now strictly fails because 9999 is not a list (doesn't start with '[' or '-')
    let err = Config::from_str(yaml).unwrap_err();
    assert!(err.message.contains("Expected list"));
}

#[test]
fn test_root_level_dash_ignored() {
    let yaml = r#"
servers:
  - host: "127.0.0.255"
- ports: [9999]
"#;
    // The second dash is at indent 0. The 'servers' list is indent 2.
    // So the list ends.
    // The root parser sees a Dash. Since Config is a struct, it sees Dash as end-of-struct (or invalid).
    // Our generated parser breaks on Dash.
    // So 'ports' is ignored.
    let config = Config::from_str(yaml).expect("Should parse partial config");
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.servers[0].host, "127.0.0.255");
    // Verify ports didn't apply to server 1
    assert_eq!(config.servers[0].ports, vec![8080]); 
}

#[test]
fn test_ambiguous_list_indentation_struct_vs_list() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: 
  - 8888
  - 6868
    server_name: "localhost"
"#;
    // This should fail because '8888' is not a valid map key for ServerConfig.
    let result = Config::from_str(yaml);
    assert!(result.is_err());
}

#[test]
fn test_inline_dashed_item() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: - 9999
"#;
    // Strict parsing now requires block list items to start on a new line.
    let err = Config::from_str(yaml).unwrap_err();
    assert!(err.message.contains("Block list item must start on a new line"));
}

#[test]
fn test_type_mismatch() {

    let yaml = r#"
servers:
  - host: "127.0.0.1"
    client_max_body_size: "not a number"
"#;
    let err = Config::from_str(yaml).unwrap_err();
    println!("{}", err);
    assert!(err.message.contains("Expected number"));
}

#[test]
fn test_list_parsing_error() {
    let yaml = r#"
servers:
  - host: "127.0.0.1"
    ports: [8080, "bad_port"]
"#;
    let err = Config::from_str(yaml).unwrap_err();
    println!("{}", err);
    assert!(err.message.contains("Expected number"));
}
