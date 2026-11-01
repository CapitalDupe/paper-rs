//! Server configuration loading and validation.
//!
//! Configuration is read from a TOML file. A missing file yields the defaults;
//! unknown keys and invalid values are rejected with a clear error listing every
//! problem found.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::Path;

use serde::Deserialize;

/// Server settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// IP address to listen on.
    pub bind_address: String,
    /// TCP port to listen on.
    pub port: u16,
    /// Maximum simultaneous players.
    pub max_players: u32,
    /// Message shown in the server list.
    pub motd: String,
    /// Authenticate players against Mojang's session servers.
    pub online_mode: bool,
    /// Packets at least this large are compressed; -1 disables compression.
    pub compression_threshold: i32,
    /// Chunk radius sent to each player.
    pub view_distance: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_owned(),
            port: 25565,
            max_players: 20,
            motd: "A paper-rs server".to_owned(),
            online_mode: true,
            compression_threshold: 256,
            view_distance: 10,
        }
    }
}

/// Errors produced while loading configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Io(io::Error),
    /// The file was not valid TOML for this configuration.
    Parse(toml::de::Error),
    /// One or more values were out of range.
    Invalid(Vec<&'static str>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "cannot read config: {err}"),
            Self::Parse(err) => write!(f, "cannot parse config: {err}"),
            Self::Invalid(problems) => write!(f, "{}", problems.join("; ")),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Parse(err) => Some(err),
            Self::Invalid(_) => None,
        }
    }
}

impl Config {
    /// Loads configuration from `path`, returning defaults if it does not exist.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_toml_str(&text),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError::Io(err)),
        }
    }

    /// Parses and validates configuration from TOML text.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    /// Checks every value, reporting all problems at once.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut problems = Vec::new();
        if self.bind_address.parse::<IpAddr>().is_err() {
            problems.push("bind_address must be an IP address");
        }
        if self.port == 0 {
            problems.push("port must be between 1 and 65535");
        }
        if !(1..=100_000).contains(&self.max_players) {
            problems.push("max_players must be between 1 and 100000");
        }
        if self.motd.chars().count() > 256 {
            problems.push("motd must be at most 256 characters");
        }
        if self.compression_threshold < -1 {
            problems.push("compression_threshold must be -1 (off) or at least 0");
        }
        if !(2..=32).contains(&self.view_distance) {
            problems.push("view_distance must be between 2 and 32");
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_secure() {
        let config = Config::default();
        assert_eq!(config.validate().is_ok(), true);
        assert!(config.online_mode);
    }

    #[test]
    fn overrides_apply_and_keep_other_defaults() {
        let config = Config::from_toml_str("port = 25570\nmax_players = 50\n").unwrap();
        assert_eq!(config.port, 25570);
        assert_eq!(config.max_players, 50);
        assert!(config.online_mode);
        assert_eq!(config.view_distance, 10);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let result = Config::from_toml_str("prot = 25570\n");
        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }

    #[test]
    fn invalid_values_are_all_reported() {
        match Config::from_toml_str("port = 0\nview_distance = 1\n") {
            Err(ConfigError::Invalid(problems)) => assert_eq!(problems.len(), 2),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_address_and_threshold() {
        let text = "bind_address = \"localhost\"\ncompression_threshold = -5\n";
        match Config::from_toml_str(text) {
            Err(ConfigError::Invalid(problems)) => assert_eq!(problems.len(), 2),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = Path::new("definitely-missing-paper-rs-server.toml");
        assert_eq!(Config::load(path).unwrap(), Config::default());
    }
}
