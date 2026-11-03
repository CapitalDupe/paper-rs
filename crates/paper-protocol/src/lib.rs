//! Minecraft Java protocol primitives for paper-rs.
//!
//! Modules are added incrementally; see `ROADMAP.md`. Nothing here opens
//! sockets: the server crate drives these primitives.
//!
//! The target game version and protocol facts are recorded in
//! `docs/adr/0003-target-minecraft-version.md` and `docs/PROTOCOL.md`.

pub mod cfb8;
pub mod codec;
pub mod compression;
pub mod frame;
pub mod handshake;
pub mod login;
pub mod packet;
pub mod status;
pub mod varint;
#[cfg(feature = "zlib")]
pub mod zlib;

/// Current implementation status of the protocol layer.
pub const STATUS: &str =
    "framing, codecs, handshake/status/login packets, compression and encryption";
