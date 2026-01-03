pub mod display;
pub mod parser;
pub mod types;
pub mod validate;

pub use parser::{ConfigParser, ParseResult, FromYaml, ConfigError};
pub use types::{Config, ServerConfig, RouteConfig};
pub use display::display_config;
pub use validate::validate_configs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_parsing_smoke() {
        // Basic smoke test to ensure the refactor didn't break everything
        let yaml = "
servers:
  - host: 127.0.0.1
    ports: [8080]
";
        let config = Config::from_str(yaml).expect("Smoke test failed");
        assert_eq!(config.servers.len(), 1);
    }
}