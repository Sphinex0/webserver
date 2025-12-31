use std::io;
use server::{config::{Config, FromYaml, display_config}, server::Server};

fn main() -> io::Result<()> {
    // 1. Read and Parse Config
    let config_path = "config.yaml";
    let raw_config = std::fs::read_to_string(config_path).expect("Failed to read config file");

    let config = Config::from_str(&raw_config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;


    // 2. Display Configuration Summary
    display_config(&config.servers);

    // 3. Initialize and Run Server
    let mut server = Server::new(config.servers)?;
    server.run()
}
