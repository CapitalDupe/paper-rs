//! Entry point for the paper-rs server prototype.
//!
//! This is intentionally only a scaffold. Networking and the game loop will be
//! added in subsequent milestones.

use std::path::Path;
use std::process::ExitCode;

use paper_server::config::Config;

fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "server.toml".to_owned());
    let config = match Config::load(Path::new(&path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("paper-rs: invalid configuration: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!("paper-rs: server scaffold (not yet playable)");
    println!("protocol layer: {}", paper_protocol::STATUS);
    println!("bind address: {}:{}", config.bind_address, config.port);
    println!("online mode: {}", config.online_mode);
    ExitCode::SUCCESS
}
