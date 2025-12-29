use std::io;

use server::{config::{ConfigParser, display_config}, lexer::Lexer, server::Server};

fn main() -> io::Result<()> {
    // 1. Read and Parse Config
    let config_path = "config.yaml";
    let raw_config = std::fs::read_to_string(config_path).expect("Failed to read config file");
    let mut tokenizer = Lexer::new(&raw_config);
    // let tokens = tokenizer.tokenize();
    match tokenizer.tokenize() {
        Ok(tokens)=>{
            // dbg!(&tokens);
            for token in &tokens {
                println!("{token}");
            }
            let config_parser = ConfigParser::new(tokens).parse().unwrap() ;
            // dbg!(config_parser);
            display_config(&config_parser);
            let mut server = Server::new(config_parser)?;
            server.run()?;

        }
        Err(err)=> println!("{err}")
    }

    // dbg!(tokens);
    


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
