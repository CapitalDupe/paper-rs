//! Isolated class loaders for plugin JARs.
//!
//! Each plugin gets its own `URLClassLoader` whose parent is the system class
//! loader, so two plugins never see each other's classes (or each other's static
//! state). Loading a class does not run its static initializers; that happens only
//! when the plugin is instantiated.

use std::path::Path;

use jni::JNIEnv;
use jni::objects::{GlobalRef, JObject, JValue};

use crate::error::JvmError;
use crate::jvm::{Jvm, check, string_from};

/// A plugin's class loader and its (loaded, not yet initialized) main class.
pub struct PluginClassLoader {
    loader: GlobalRef,
    main_class: GlobalRef,
    main_class_name: String,
}

impl PluginClassLoader {
    /// The `URLClassLoader` object.
    pub fn loader(&self) -> &GlobalRef {
        &self.loader
    }

    /// The `java.lang.Class` object of the main class.
    pub fn main_class(&self) -> &GlobalRef {
        &self.main_class
    }

    /// The main class name the loader was asked for.
    pub fn main_class_name(&self) -> &str {
        &self.main_class_name
    }

    /// The name Java reports for the loaded main class.
    pub fn loaded_class_name(&self, jvm: &Jvm) -> Result<String, JvmError> {
        jvm.with_env(|env| {
            let call = env.call_method(
                self.main_class.as_obj(),
                "getName",
                "()Ljava/lang/String;",
                &[],
            );
            let value = check(env, call)?;
            let object = check(env, value.l())?;
            string_from(env, object)
        })
    }

    /// Closes the class loader, releasing the JAR file.
    pub fn close(&self, jvm: &Jvm) -> Result<(), JvmError> {
        jvm.with_env(|env| {
            let call = env.call_method(self.loader.as_obj(), "close", "()V", &[]);
            check(env, call).map(|_| ())
        })
    }
}

fn load(env: &mut JNIEnv<'_>, jar: &Path, main_class: &str) -> Result<PluginClassLoader, JvmError> {
    let Some(path) = jar.to_str() else {
        return Err(JvmError::BadConfig(
            "the plugin jar path must be valid UTF-8".to_owned(),
        ));
    };
    let path = env.new_string(path);
    let path = check(env, path)?;
    let file = env.new_object(
        "java/io/File",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&path)],
    );
    let file = check(env, file)?;
    let uri = env.call_method(&file, "toURI", "()Ljava/net/URI;", &[]);
    let uri = check(env, uri)?;
    let uri = check(env, uri.l())?;
    let url = env.call_method(&uri, "toURL", "()Ljava/net/URL;", &[]);
    let url = check(env, url)?;
    let url = check(env, url.l())?;
    let urls = env.new_object_array(1, "java/net/URL", JObject::null());
    let urls = check(env, urls)?;
    let stored = env.set_object_array_element(&urls, 0, &url);
    check(env, stored)?;
    let parent = env.call_static_method(
        "java/lang/ClassLoader",
        "getSystemClassLoader",
        "()Ljava/lang/ClassLoader;",
        &[],
    );
    let parent = check(env, parent)?;
    let parent = check(env, parent.l())?;
    let loader = env.new_object(
        "java/net/URLClassLoader",
        "([Ljava/net/URL;Ljava/lang/ClassLoader;)V",
        &[JValue::Object(&urls), JValue::Object(&parent)],
    );
    let loader = check(env, loader)?;

    let name = env.new_string(main_class);
    let name = check(env, name)?;
    let class = env.call_method(
        &loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&name)],
    );
    let class = match check(env, class) {
        Ok(value) => value,
        Err(err) => {
            let _ = env.call_method(&loader, "close", "()V", &[]);
            let _ = env.exception_clear();
            return Err(err);
        }
    };
    let class = check(env, class.l())?;
    let loader_ref = env.new_global_ref(&loader);
    let loader_ref = check(env, loader_ref)?;
    let class_ref = env.new_global_ref(&class);
    let class_ref = check(env, class_ref)?;
    Ok(PluginClassLoader {
        loader: loader_ref,
        main_class: class_ref,
        main_class_name: main_class.to_owned(),
    })
}

impl Jvm {
    /// Creates an isolated class loader for `jar` and loads `main_class` from it.
    ///
    /// On failure the class loader is closed again, so no file handle leaks.
    pub fn load_plugin_class(
        &self,
        jar: &Path,
        main_class: &str,
    ) -> Result<PluginClassLoader, JvmError> {
        self.with_env(|env| load(env, jar, main_class))
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use super::*;
    use crate::jvm::JvmConfig;

    fn jvm() -> Option<&'static Jvm> {
        let config = JvmConfig {
            max_heap_mb: Some(256),
            ..JvmConfig::default()
        };
        match Jvm::global(&config) {
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

    /// Path of a jar built by `scripts/build-test-plugins.sh`, if it exists.
    fn test_jar(name: &str) -> Option<PathBuf> {
        let dir = match env::var_os("MAGNA_TEST_PLUGINS_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-plugins"),
        };
        let jar = dir.join(format!("{name}.jar"));
        if jar.is_file() {
            return Some(jar);
        }
        assert!(
            env::var_os("PAPER_REQUIRE_JVM").is_none(),
            "test plugin {name}.jar was not built"
        );
        eprintln!("skipping: {} not built", jar.display());
        None
    }

    #[test]
    fn parent_delegation_finds_jdk_classes_even_in_an_empty_loader() {
        let Some(jvm) = jvm() else {
            return;
        };
        let loaded = jvm
            .load_plugin_class(Path::new("/nonexistent/plugin.jar"), "java.lang.Object")
            .unwrap();
        assert_eq!(loaded.loaded_class_name(jvm).unwrap(), "java.lang.Object");
        loaded.close(jvm).unwrap();
    }

    #[test]
    fn a_missing_class_is_a_java_error_not_a_crash() {
        let Some(jvm) = jvm() else {
            return;
        };
        let result = jvm.load_plugin_class(Path::new("/nonexistent/plugin.jar"), "example.Nope");
        match result {
            Err(JvmError::Java(message)) => {
                assert!(message.contains("ClassNotFoundException"), "{message}");
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("loading a missing class succeeded"),
        }
        assert!(jvm.java_version().is_ok());
    }

    #[test]
    fn loads_the_main_class_from_a_real_plugin_jar() {
        let Some(jvm) = jvm() else {
            return;
        };
        let Some(jar) = test_jar("hello") else {
            return;
        };
        let loaded = jvm.load_plugin_class(&jar, "example.Hello").unwrap();
        assert_eq!(loaded.main_class_name(), "example.Hello");
        assert_eq!(loaded.loaded_class_name(jvm).unwrap(), "example.Hello");
        loaded.close(jvm).unwrap();
    }

    #[test]
    fn each_load_gets_its_own_isolated_class() {
        let Some(jvm) = jvm() else {
            return;
        };
        let Some(jar) = test_jar("hello") else {
            return;
        };
        let first = jvm.load_plugin_class(&jar, "example.Hello").unwrap();
        let second = jvm.load_plugin_class(&jar, "example.Hello").unwrap();
        let same = jvm
            .with_env(|env| {
                let result =
                    env.is_same_object(first.main_class().as_obj(), second.main_class().as_obj());
                check(env, result)
            })
            .unwrap();
        assert!(!same, "two loaders must not share a class");
        first.close(jvm).unwrap();
        second.close(jvm).unwrap();
    }
}
