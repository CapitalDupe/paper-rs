//! Errors from the embedded JVM.

use std::error::Error;
use std::fmt;

/// Errors produced by the JVM bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JvmError {
    /// The JVM configuration is invalid.
    BadConfig(String),
    /// The JVM could not be located or started.
    Start(String),
    /// A JNI call failed for a reason other than a Java exception.
    Jni(String),
    /// Java code threw an exception; the text is `Throwable.toString()`.
    Java(String),
}

impl fmt::Display for JvmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadConfig(why) => write!(f, "invalid JVM configuration: {why}"),
            Self::Start(why) => write!(f, "cannot start the JVM: {why}"),
            Self::Jni(why) => write!(f, "JNI error: {why}"),
            Self::Java(why) => write!(f, "Java exception: {why}"),
        }
    }
}

impl Error for JvmError {}
