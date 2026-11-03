//! Plugin discovery, metadata parsing, and dependency ordering.
//!
//! This crate is pure Rust and never touches the JVM: it reads each JAR's
//! `plugin.yml`, validates it, and decides which plugins load and in what order.
//! Loading the classes themselves happens in `paper-jvm`, because that is the one
//! part that has to run inside Java.
//!
//! Plugin JARs and their metadata are untrusted input. Everything read from a JAR
//! is size-limited, and YAML aliases are rejected so a tiny file cannot expand
//! into a huge structure.

pub mod discovery;
pub mod meta;
pub mod resolve;
