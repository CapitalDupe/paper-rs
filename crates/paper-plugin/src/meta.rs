//! Parsing and validating `plugin.yml`.

use std::error::Error;
use std::fmt;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;
use yaml_rust2::{Yaml, YamlLoader};

/// Largest `plugin.yml` accepted, in bytes.
pub const MAX_META_BYTES: usize = 128 * 1024;

const MAX_NODES: usize = 10_000;

/// When a plugin wants to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadOrder {
    /// Before worlds load.
    Startup,
    /// After worlds load (the default).
    PostWorld,
}

/// The parts of `plugin.yml` the server acts on. Other keys are ignored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMeta {
    /// Unique plugin name.
    pub name: String,
    /// Plugin version text.
    pub version: String,
    /// Fully qualified main class name.
    pub main: String,
    /// The `api-version` the plugin targets, if given.
    pub api_version: Option<String>,
    /// Load phase.
    pub load: LoadOrder,
    /// Plugins that must be present and load first.
    pub depend: Vec<String>,
    /// Plugins that load first if present.
    pub softdepend: Vec<String>,
    /// Plugins that must load after this one, if present.
    pub loadbefore: Vec<String>,
    /// Free-text description.
    pub description: Option<String>,
    /// Authors.
    pub authors: Vec<String>,
}

/// Why a `plugin.yml` was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaError {
    /// The file is larger than [`MAX_META_BYTES`].
    TooLarge,
    /// The YAML could not be parsed.
    Yaml(String),
    /// The YAML uses a feature that is refused for safety.
    Unsafe(&'static str),
    /// The top level is not a mapping.
    NotAMapping,
    /// A required key is missing.
    Missing(&'static str),
    /// A key has an invalid value: `(key, why)`.
    Invalid(&'static str, String),
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("plugin.yml is too large"),
            Self::Yaml(why) => write!(f, "plugin.yml is not valid YAML: {why}"),
            Self::Unsafe(why) => write!(f, "plugin.yml rejected: {why}"),
            Self::NotAMapping => f.write_str("plugin.yml must be a mapping"),
            Self::Missing(key) => write!(f, "plugin.yml is missing `{key}`"),
            Self::Invalid(key, why) => write!(f, "plugin.yml `{key}` is invalid: {why}"),
        }
    }
}

impl Error for MetaError {}

struct Guard {
    aliases: bool,
    nodes: usize,
}

impl MarkedEventReceiver for Guard {
    fn on_event(&mut self, event: Event, _mark: Marker) {
        match event {
            Event::Alias(_) => self.aliases = true,
            Event::Scalar(..) | Event::SequenceStart(..) | Event::MappingStart(..) => {
                self.nodes += 1;
            }
            _ => {}
        }
    }
}

fn guard(text: &str) -> Result<(), MetaError> {
    let mut guard = Guard {
        aliases: false,
        nodes: 0,
    };
    let mut parser = Parser::new_from_str(text);
    parser
        .load(&mut guard, false)
        .map_err(|err| MetaError::Yaml(err.to_string()))?;
    if guard.aliases {
        return Err(MetaError::Unsafe("YAML aliases are not supported"));
    }
    if guard.nodes > MAX_NODES {
        return Err(MetaError::Unsafe("too many YAML nodes"));
    }
    Ok(())
}

/// Whether `name` is an acceptable plugin name.
pub fn is_valid_plugin_name(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
    !name.is_empty() && name.len() <= 64 && name.chars().all(allowed)
}

/// Whether `name` is a plausible fully qualified Java class name.
pub fn is_valid_class_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    name.split('.').all(|segment| {
        let mut chars = segment.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
        first_ok && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    })
}

fn scalar_string(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(text) | Yaml::Real(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn required(doc: &Yaml, key: &'static str) -> Result<String, MetaError> {
    let value = &doc[key];
    if matches!(value, Yaml::BadValue | Yaml::Null) {
        return Err(MetaError::Missing(key));
    }
    scalar_string(value).ok_or_else(|| MetaError::Invalid(key, "must be a plain value".to_owned()))
}

fn optional(doc: &Yaml, key: &'static str) -> Result<Option<String>, MetaError> {
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(None),
        value => scalar_string(value)
            .map(Some)
            .ok_or_else(|| MetaError::Invalid(key, "must be a plain value".to_owned())),
    }
}

fn string_list(doc: &Yaml, key: &'static str) -> Result<Vec<String>, MetaError> {
    let bad = || MetaError::Invalid(key, "must be a name or a list of names".to_owned());
    match &doc[key] {
        Yaml::BadValue | Yaml::Null => Ok(Vec::new()),
        Yaml::Array(items) => items
            .iter()
            .map(|item| scalar_string(item).ok_or_else(bad))
            .collect(),
        other => scalar_string(other).map(|text| vec![text]).ok_or_else(bad),
    }
}

fn plugin_names(doc: &Yaml, key: &'static str) -> Result<Vec<String>, MetaError> {
    let names = string_list(doc, key)?;
    if let Some(bad) = names.iter().find(|name| !is_valid_plugin_name(name)) {
        return Err(MetaError::Invalid(key, format!("bad plugin name {bad:?}")));
    }
    Ok(names)
}

/// Parses and validates the text of a `plugin.yml`.
pub fn parse_plugin_yml(text: &str) -> Result<PluginMeta, MetaError> {
    if text.len() > MAX_META_BYTES {
        return Err(MetaError::TooLarge);
    }
    guard(text)?;
    let docs = YamlLoader::load_from_str(text).map_err(|err| MetaError::Yaml(err.to_string()))?;
    let Some(doc) = docs.first() else {
        return Err(MetaError::NotAMapping);
    };
    if doc.as_hash().is_none() {
        return Err(MetaError::NotAMapping);
    }

    let name = required(doc, "name")?;
    if !is_valid_plugin_name(&name) {
        return Err(MetaError::Invalid(
            "name",
            "use only letters, digits, '_', '.' and '-'".to_owned(),
        ));
    }
    let main = required(doc, "main")?;
    if !is_valid_class_name(&main) {
        return Err(MetaError::Invalid(
            "main",
            "not a valid class name".to_owned(),
        ));
    }
    if main.starts_with("org.bukkit.") {
        return Err(MetaError::Invalid(
            "main",
            "the main class may not be in org.bukkit".to_owned(),
        ));
    }
    let version = required(doc, "version")?;
    let load = match optional(doc, "load")? {
        None => LoadOrder::PostWorld,
        Some(text) => match text.to_ascii_uppercase().as_str() {
            "STARTUP" => LoadOrder::Startup,
            "POSTWORLD" => LoadOrder::PostWorld,
            _ => {
                return Err(MetaError::Invalid(
                    "load",
                    "must be STARTUP or POSTWORLD".to_owned(),
                ));
            }
        },
    };
    let mut authors = string_list(doc, "authors")?;
    authors.extend(optional(doc, "author")?);

    Ok(PluginMeta {
        name,
        version,
        main,
        api_version: optional(doc, "api-version")?,
        load,
        depend: plugin_names(doc, "depend")?,
        softdepend: plugin_names(doc, "softdepend")?,
        loadbefore: plugin_names(doc, "loadbefore")?,
        description: optional(doc, "description")?,
        authors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "\
name: Example
version: 1.2.3
main: com.example.ExamplePlugin
api-version: \"1.20\"
load: STARTUP
description: Does things
author: Alice
authors: [Bob, Carol]
depend: [Core]
softdepend:
  - Extras
loadbefore: Later
commands:
  hello:
    description: says hello
permissions:
  example.use:
    default: true
";

    #[test]
    fn parses_a_full_plugin_yml() {
        let meta = parse_plugin_yml(FULL).unwrap();
        assert_eq!(meta.name, "Example");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(meta.main, "com.example.ExamplePlugin");
        assert_eq!(meta.api_version.as_deref(), Some("1.20"));
        assert_eq!(meta.load, LoadOrder::Startup);
        assert_eq!(meta.description.as_deref(), Some("Does things"));
        assert_eq!(meta.authors, ["Bob", "Carol", "Alice"]);
        assert_eq!(meta.depend, ["Core"]);
        assert_eq!(meta.softdepend, ["Extras"]);
        assert_eq!(meta.loadbefore, ["Later"]);
    }

    #[test]
    fn applies_defaults() {
        let meta = parse_plugin_yml("name: A\nmain: a.B\nversion: 1.0\n").unwrap();
        assert_eq!(meta.load, LoadOrder::PostWorld);
        assert_eq!(meta.version, "1.0");
        assert!(meta.depend.is_empty() && meta.authors.is_empty());
        assert_eq!(meta.api_version, None);
    }

    #[test]
    fn numeric_versions_keep_their_text() {
        let meta = parse_plugin_yml("name: A\nmain: a.B\nversion: 2\n").unwrap();
        assert_eq!(meta.version, "2");
    }

    #[test]
    fn requires_name_main_and_version() {
        let missing = |text| parse_plugin_yml(text).unwrap_err();
        assert_eq!(
            missing("main: a.B\nversion: 1\n"),
            MetaError::Missing("name")
        );
        assert_eq!(missing("name: A\nversion: 1\n"), MetaError::Missing("main"));
        assert_eq!(
            missing("name: A\nmain: a.B\n"),
            MetaError::Missing("version")
        );
    }

    #[test]
    fn rejects_bad_names_and_classes() {
        let invalid = |text| matches!(parse_plugin_yml(text), Err(MetaError::Invalid(..)));
        assert!(invalid("name: my plugin\nmain: a.B\nversion: 1\n"));
        assert!(invalid("name: ../evil\nmain: a.B\nversion: 1\n"));
        assert!(invalid("name: A\nmain: a..B\nversion: 1\n"));
        assert!(invalid("name: A\nmain: 1a.B\nversion: 1\n"));
        assert!(invalid("name: A\nmain: org.bukkit.Thing\nversion: 1\n"));
        assert!(invalid("name: A\nmain: a.B\nversion: 1\nload: SOMETIME\n"));
        assert!(invalid(
            "name: A\nmain: a.B\nversion: 1\ndepend: [\"bad name\"]\n"
        ));
    }

    #[test]
    fn load_order_is_case_insensitive() {
        let meta = parse_plugin_yml("name: A\nmain: a.B\nversion: 1\nload: startup\n").unwrap();
        assert_eq!(meta.load, LoadOrder::Startup);
    }

    #[test]
    fn rejects_non_mappings_and_bad_yaml() {
        assert_eq!(parse_plugin_yml("- a\n- b\n"), Err(MetaError::NotAMapping));
        assert_eq!(parse_plugin_yml(""), Err(MetaError::NotAMapping));
        assert!(matches!(
            parse_plugin_yml("a: [1, 2\n"),
            Err(MetaError::Yaml(_))
        ));
    }

    #[test]
    fn rejects_yaml_aliases() {
        let text = "name: A\nmain: a.B\nversion: 1\nx: &anchor [1, 2]\ny: *anchor\n";
        assert!(matches!(parse_plugin_yml(text), Err(MetaError::Unsafe(_))));
    }

    #[test]
    fn rejects_oversized_files() {
        let text = format!(
            "name: A\nmain: a.B\nversion: 1\nx: {}\n",
            "a".repeat(MAX_META_BYTES)
        );
        assert_eq!(parse_plugin_yml(&text), Err(MetaError::TooLarge));
    }

    #[test]
    fn name_and_class_validators() {
        assert!(is_valid_plugin_name("Essentials-X_2.0"));
        assert!(!is_valid_plugin_name(""));
        assert!(!is_valid_plugin_name(&"a".repeat(65)));
        assert!(is_valid_class_name("com.example.Main$Inner"));
        assert!(!is_valid_class_name("com/example/Main"));
        assert!(!is_valid_class_name(""));
    }
}
