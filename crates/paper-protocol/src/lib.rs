//! Minecraft Java protocol primitives for paper-rs.
//!
//! Modules are added incrementally; see `ROADMAP.md`. Nothing here opens
//! sockets: the server crate drives these primitives.

pub mod codec;
pub mod error;
pub mod frame;
pub mod handshake;
pub mod login;
pub mod status;
pub mod varint;

/// Current implementation status of the protocol layer.
pub const STATUS: &str = "scaffold";
