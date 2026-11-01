//! Logging setup and secret redaction.
//!
//! Logs are structured through `tracing`. Anything sensitive (shared secrets,
//! verify tokens, session ids) must be wrapped in [`Redacted`] before it is
//! passed to a log macro, so it can never be printed by accident.

use std::error::Error;
use std::fmt;

use tracing_subscriber::EnvFilter;

/// Wraps a value so it is never printed, in either `Debug` or `Display` form.
pub struct Redacted<T>(pub T);

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Errors produced while starting logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingError {
    /// The log filter string could not be parsed.
    BadFilter,
    /// A global logger was already installed.
    AlreadyInitialized,
}

impl fmt::Display for LoggingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadFilter => f.write_str("invalid log filter"),
            Self::AlreadyInitialized => f.write_str("logging is already initialized"),
        }
    }
}

impl Error for LoggingError {}

/// Installs the global logger using `level` (a level name or filter directive).
pub fn init(level: &str) -> Result<(), LoggingError> {
    let Ok(filter) = EnvFilter::try_new(level) else {
        return Err(LoggingError::BadFilter);
    };
    let result = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    result.map_err(|_| LoggingError::AlreadyInitialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_never_prints_the_value() {
        let secret = Redacted("hunter2");
        assert_eq!(format!("{secret}"), "[redacted]");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{:?}", Redacted(vec![1, 2, 3])), "[redacted]");
    }

    #[test]
    fn rejects_an_invalid_filter() {
        assert_eq!(init("paper=loud"), Err(LoggingError::BadFilter));
    }
}
