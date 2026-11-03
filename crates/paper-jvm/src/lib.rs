//! Embeds a JVM in the Rust process.
//!
//! Java is used only where it has to be: plugins are Java bytecode, so they need
//! a JVM. Everything else in the server stays in Rust. This crate starts the JVM,
//! attaches threads to it, and turns Java exceptions into Rust errors so a
//! misbehaving plugin cannot take the server down.
//!
//! The JVM is located at runtime from `JAVA_HOME` (or the `java` on `PATH`), so
//! building the server does not require a JDK. Only one JVM can exist per process
//! and it cannot be restarted, so use [`Jvm::global`].

pub mod classloader;
pub mod error;
pub mod jvm;

pub use classloader::PluginClassLoader;
pub use error::JvmError;
pub use jvm::{Jvm, JvmConfig};
