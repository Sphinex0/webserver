use std::io;
use server::{config::{Config, FromYaml, display_config, validate_configs}, server::Server};

fn main() -> io::Result<()> {
    // 1. Read and Parse Config
    let config_path = "config.yaml";
    let raw_config = std::fs::read_to_string(config_path).expect("Failed to read config file");

    let config = Config::from_str(&raw_config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // 2. Validate Configuration
    let valid_servers = validate_configs(config.servers);
    if valid_servers.is_empty() {
        eprintln!("❌ \x1b[1;31mCritical Error\x1b[0m: No valid server configurations remain after validation. Exiting.");
        return Err(io::Error::new(io::ErrorKind::InvalidData, "No valid server configurations"));
    }

    // 3. Display Configuration Summary
    display_config(&valid_servers);

    // 4. Initialize and Run Server
    let mut server = Server::new(valid_servers)?;
    server.run()
}
