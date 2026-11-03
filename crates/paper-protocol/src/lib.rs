//! Minecraft Java protocol primitives for paper-rs.
//!
//! Modules are added incrementally; see `ROADMAP.md`. Nothing here opens
//! sockets: the server crate drives these primitives.
//!
//! The target game version and protocol facts are recorded in
//! `docs/adr/0003-target-minecraft-version.md` and `docs/PROTOCOL.md`.

pub mod codec;
pub mod frame;
pub mod packet;
pub mod varint;

/// Current implementation status of the protocol layer.
pub const STATUS: &str = "framing and codec primitives";
