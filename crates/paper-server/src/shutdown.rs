//! Operating-system shutdown signals.

/// Waits for a termination request and returns the name of the signal received.
///
/// Listens for Ctrl-C everywhere and for SIGTERM on Unix. If signal handlers
/// cannot be installed the future never completes, so the server keeps running
/// rather than shutting down by accident.
pub async fn wait_for_signal() -> &'static str {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            return tokio::select! {
                _ = tokio::signal::ctrl_c() => "interrupt",
                _ = terminate.recv() => "terminate",
            };
        }
    }
    if tokio::signal::ctrl_c().await.is_err() {
        tracing::warn!("cannot listen for ctrl-c; the shutdown signal is disabled");
        std::future::pending::<()>().await;
    }
    "interrupt"
}
