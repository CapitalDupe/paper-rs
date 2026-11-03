//! Starting the JVM and calling into it.

use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

use jni::objects::{JObject, JString, JThrowable, JValue};
use jni::{AttachGuard, InitArgsBuilder, JNIEnv, JNIVersion, JavaVM};

use crate::error::JvmError;

/// How to start the JVM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JvmConfig {
    /// Maximum heap size in megabytes (`-Xmx`); `None` uses the JVM default.
    pub max_heap_mb: Option<u32>,
    /// Entries for the JVM class path (JARs or directories).
    pub class_path: Vec<PathBuf>,
    /// Enable extra JNI argument checking (slow; for development).
    pub check_jni: bool,
}

/// Turns a configuration into JVM command-line options.
pub fn build_options(config: &JvmConfig) -> Result<Vec<String>, JvmError> {
    let mut options = vec!["-Djava.awt.headless=true".to_owned()];
    if let Some(megabytes) = config.max_heap_mb {
        if megabytes == 0 {
            return Err(JvmError::BadConfig("max_heap_mb must be positive".to_owned()));
        }
        options.push(format!("-Xmx{megabytes}m"));
    }
    if !config.class_path.is_empty() {
        let Ok(joined) = env::join_paths(&config.class_path) else {
            return Err(JvmError::BadConfig(
                "a class path entry contains the path separator".to_owned(),
            ));
        };
        let Some(joined) = joined.to_str() else {
            return Err(JvmError::BadConfig(
                "class path entries must be valid UTF-8".to_owned(),
            ));
        };
        options.push(format!("-Djava.class.path={joined}"));
    }
    if config.check_jni {
        options.push("-Xcheck:jni".to_owned());
    }
    Ok(options)
}

/// Converts the result of a JNI call, turning a pending Java exception into
/// [`JvmError::Java`] and clearing it so the thread stays usable.
pub fn check<T>(env: &mut JNIEnv<'_>, result: jni::errors::Result<T>) -> Result<T, JvmError> {
    match result {
        Ok(value) => Ok(value),
        Err(jni::errors::Error::JavaException) => Err(JvmError::Java(take_exception(env))),
        Err(other) => {
            let _ = env.exception_clear();
            Err(JvmError::Jni(other.to_string()))
        }
    }
}

/// Removes the pending Java exception (if any) and describes it.
pub fn take_exception(env: &mut JNIEnv<'_>) -> String {
    let Ok(throwable) = env.exception_occurred() else {
        let _ = env.exception_clear();
        return "unknown Java exception".to_owned();
    };
    let _ = env.exception_clear();
    describe_throwable(env, &throwable)
}

fn describe_throwable(env: &mut JNIEnv<'_>, throwable: &JThrowable<'_>) -> String {
    let call = env.call_method(throwable, "toString", "()Ljava/lang/String;", &[]);
    let Ok(value) = call else {
        let _ = env.exception_clear();
        return "Java exception (could not be described)".to_owned();
    };
    let Ok(object) = value.l() else {
        return "Java exception (could not be described)".to_owned();
    };
    match string_from(env, object) {
        Ok(text) => text,
        Err(_) => "Java exception (could not be described)".to_owned(),
    }
}

/// Reads a `java.lang.String` object into a Rust string.
pub fn string_from(env: &mut JNIEnv<'_>, object: JObject<'_>) -> Result<String, JvmError> {
    if object.is_null() {
        return Err(JvmError::Jni("unexpected null string".to_owned()));
    }
    let text = JString::from(object);
    let java_str = env.get_string(&text);
    let java_str = check(env, java_str)?;
    Ok(String::from(java_str))
}

fn system_property(env: &mut JNIEnv<'_>, name: &str) -> Result<String, JvmError> {
    let key = env.new_string(name);
    let key = check(env, key)?;
    let result = env.call_static_method(
        "java/lang/System",
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[JValue::Object(&key)],
    );
    let value = check(env, result)?;
    let object = check(env, value.l())?;
    string_from(env, object)
}

/// A running JVM.
pub struct Jvm {
    vm: JavaVM,
}

static GLOBAL: OnceLock<Result<Jvm, JvmError>> = OnceLock::new();

impl Jvm {
    /// Starts a JVM. A process can start at most one, ever.
    pub fn start(config: &JvmConfig) -> Result<Self, JvmError> {
        let mut builder = InitArgsBuilder::new().version(JNIVersion::V8);
        for option in build_options(config)? {
            builder = builder.option(option);
        }
        let args = builder
            .build()
            .map_err(|err| JvmError::BadConfig(err.to_string()))?;
        let vm = JavaVM::new(args).map_err(|err| JvmError::Start(err.to_string()))?;
        Ok(Self { vm })
    }

    /// The process-wide JVM, started on first use with `config`.
    ///
    /// Later calls return the same JVM and ignore their `config`; a failed start
    /// is remembered and returned again.
    pub fn global(config: &JvmConfig) -> Result<&'static Jvm, &'static JvmError> {
        GLOBAL.get_or_init(|| Self::start(config)).as_ref()
    }

    /// Attaches the current thread; it detaches when the guard is dropped.
    pub fn attach(&self) -> Result<AttachGuard<'_>, JvmError> {
        self.vm
            .attach_current_thread()
            .map_err(|err| JvmError::Jni(err.to_string()))
    }

    /// Runs `f` with the current thread attached.
    pub fn with_env<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv<'_>) -> Result<T, JvmError>,
    ) -> Result<T, JvmError> {
        let mut guard = self.attach()?;
        f(&mut guard)
    }

    /// The JVM's `java.version` system property.
    pub fn java_version(&self) -> Result<String, JvmError> {
        self.with_env(|env| system_property(env, "java.version"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> JvmConfig {
        JvmConfig {
            max_heap_mb: Some(256),
            ..JvmConfig::default()
        }
    }

    /// The shared test JVM, or `None` if no JVM is installed.
    ///
    /// Tests skip when there is no JVM unless `PAPER_REQUIRE_JVM` is set (CI sets
    /// it, so a missing JVM there is a failure rather than a silent skip).
    fn jvm() -> Option<&'static Jvm> {
        match Jvm::global(&test_config()) {
            Ok(jvm) => Some(jvm),
            Err(err) => {
                assert!(
                    env::var_os("PAPER_REQUIRE_JVM").is_none(),
                    "a JVM is required: {err}"
                );
                eprintln!("skipping: {err}");
                None
            }
        }
    }

    #[test]
    fn default_options_are_minimal() {
        let options = build_options(&JvmConfig::default()).unwrap();
        assert_eq!(options, ["-Djava.awt.headless=true"]);
    }

    #[test]
    fn options_reflect_the_configuration() {
        let config = JvmConfig {
            max_heap_mb: Some(512),
            class_path: vec![PathBuf::from("a.jar"), PathBuf::from("lib")],
            check_jni: true,
        };
        let joined = env::join_paths(&config.class_path).unwrap();
        let options = build_options(&config).unwrap();
        assert!(options.contains(&"-Xmx512m".to_owned()));
        let expected = format!("-Djava.class.path={}", joined.to_str().unwrap());
        assert!(options.contains(&expected));
        assert!(options.contains(&"-Xcheck:jni".to_owned()));
    }

    #[test]
    fn zero_heap_is_rejected() {
        let config = JvmConfig {
            max_heap_mb: Some(0),
            ..JvmConfig::default()
        };
        assert!(matches!(build_options(&config), Err(JvmError::BadConfig(_))));
    }

    #[cfg(unix)]
    #[test]
    fn class_path_entries_with_the_separator_are_rejected() {
        let config = JvmConfig {
            class_path: vec![PathBuf::from("a:b")],
            ..JvmConfig::default()
        };
        assert!(matches!(build_options(&config), Err(JvmError::BadConfig(_))));
    }

    #[test]
    fn reports_the_java_version() {
        let Some(jvm) = jvm() else {
            return;
        };
        let version = jvm.java_version().unwrap();
        assert!(version.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }

    #[test]
    fn java_exceptions_become_errors_and_the_jvm_stays_usable() {
        let Some(jvm) = jvm() else {
            return;
        };
        let result = jvm.with_env(|env| {
            let text = env.new_string("not a number");
            let text = check(env, text)?;
            let call = env.call_static_method(
                "java/lang/Integer",
                "parseInt",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&text)],
            );
            check(env, call).map(|_| ())
        });
        match result {
            Err(JvmError::Java(message)) => {
                assert!(message.contains("NumberFormatException"), "{message}");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(jvm.java_version().is_ok());
    }

    #[test]
    fn the_heap_limit_is_applied() {
        let Some(jvm) = jvm() else {
            return;
        };
        let max = jvm
            .with_env(|env| {
                let runtime = env.call_static_method(
                    "java/lang/Runtime",
                    "getRuntime",
                    "()Ljava/lang/Runtime;",
                    &[],
                );
                let runtime = check(env, runtime)?;
                let runtime = check(env, runtime.l())?;
                let call = env.call_method(&runtime, "maxMemory", "()J", &[]);
                let value = check(env, call)?;
                check(env, value.j())
            })
            .unwrap();
        assert!(max > 0 && max <= 256 * 1024 * 1024, "max heap was {max}");
    }

    #[test]
    fn works_from_several_threads_at_once() {
        let Some(jvm) = jvm() else {
            return;
        };
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| assert!(jvm.java_version().is_ok()));
            }
        });
    }
}
