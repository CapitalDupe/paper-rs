//! Entry point for the Magna MC server prototype.
//!
//! The server accepts status pings and offline-mode logins; gameplay is not
//! implemented yet. It runs until it receives Ctrl-C or SIGTERM, then shuts down
//! cleanly.

use std::path::Path;
use std::process::ExitCode;

use paper_server::config::Config;
use paper_server::logging;
use paper_server::server;
use paper_server::shutdown;

#[tokio::main]
async fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "server.toml".to_owned());
    let config = match Config::load(Path::new(&path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{}: invalid configuration: {err}", paper_server::SERVER_NAME);
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = logging::init(&config.log_level) {
        eprintln!("{}: cannot start logging: {err}", paper_server::SERVER_NAME);
        return ExitCode::FAILURE;
    }
    tracing::info!("{} starting (not yet playable)", paper_server::SERVER_NAME);
    tracing::info!("protocol layer: {}", paper_protocol::STATUS);
    if config.online_mode {
        tracing::warn!("online mode is not supported yet; logins will be refused");
    }
    let stop = async {
        let signal = shutdown::wait_for_signal().await;
        tracing::info!("received {signal} signal");
    };
    match server::run(config, stop).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("server stopped: {err}");
            ExitCode::FAILURE
        }
    }
}
