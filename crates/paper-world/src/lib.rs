//! World data model for the Magna MC server prototype.
//!
//! This crate is pure data and logic: no I/O, no networking. Chunks store blocks
//! in palette-compressed sections; persistence lives in `paper-storage`.
//!
//! Block state ids are opaque numbers here. Mapping them to named blocks and to
//! the wire format depends on the target Minecraft version (ADR 0003).

pub mod block;
pub mod chunk;
pub mod entity;
pub mod generator;
pub mod manager;
pub mod packed;
pub mod palette;
pub mod player;
pub mod pos;
pub mod tracker;
pub mod world;
