//! Minecraft Java protocol primitives for paper-rs.
//!
//! Modules are added incrementally; see `ROADMAP.md`. Nothing here opens
//! sockets: the server crate drives these primitives.

pub mod frame;
pub mod varint;

/// Current implementation status of the protocol layer.
pub const STATUS: &str = "scaffold";
