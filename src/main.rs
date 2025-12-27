use std::io;

use server::{
    config::{ConfigParser, display_config}, lexer::{Tokenizer, tokens::Token}, server::Server
};

fn main() -> io::Result<()> {
    // 1. Read and Parse Config
    let config_path = "config.yaml";
    let raw_config = std::fs::read_to_string(config_path).expect("Failed to read config file");
    let tokenizer = Tokenizer::new(&raw_config);
    // while let Some(token) = tokenizer.next_token() {
    //     if token == Token::EOF {
    //         break;
    //     }
    //     dbg!(token);
    // }
    // Assume ConfigParser returns Vec<ServerConfig>
    // let mut parser = ConfigParser::new(raw_config);
    // let configs = parser.parse();

    // display_config(&configs);

    // // 2. Initialize Server with all configs
    // let mut server = Server::new(configs)?;

    // // 3. Run the event loop
    // server.run()
    Ok(())
}


