//! Library half of the Magna MC server prototype.
//!
//! Configuration, logging, networking, the connection state machine, the tick
//! loop, and shutdown handling live here. World and gameplay arrive in later
//! milestones.

/// The server's brand name, shown in the status response and logs.
pub const SERVER_NAME: &str = "Magna MC";

pub mod config;
pub mod logging;
pub mod net;
pub mod server;
pub mod session;
pub mod shutdown;
pub mod tick;
