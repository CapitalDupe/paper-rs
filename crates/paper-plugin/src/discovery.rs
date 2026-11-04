//! Finding plugin JARs and reading their metadata.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};

use zip::ZipArchive;
use zip::result::ZipError;

use crate::meta::{MAX_META_BYTES, PluginMeta, parse_plugin_yml};

/// A plugin JAR with valid metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// Path of the JAR.
    pub jar: PathBuf,
    /// Its parsed `plugin.yml`.
    pub meta: PluginMeta,
}

/// A JAR that could not be used, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryProblem {
    /// Path of the JAR.
    pub jar: PathBuf,
    /// Human-readable reason.
    pub reason: String,
}

/// The result of scanning a plugins directory.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Usable plugins, in file-name order.
    pub plugins: Vec<DiscoveredPlugin>,
    /// JARs that were skipped.
    pub problems: Vec<DiscoveryProblem>,
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<String>, String> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(format!("cannot read {name}: {err}")),
    };
    let mut bytes = Vec::new();
    let limit = MAX_META_BYTES as u64 + 1;
    entry
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("cannot read {name}: {err}"))?;
    if bytes.len() > MAX_META_BYTES {
        return Err(format!("{name} is too large"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| format!("{name} is not valid UTF-8"))
}

/// Reads and validates the `plugin.yml` inside a JAR.
///
/// At most [`MAX_META_BYTES`] are read from the entry, whatever its header claims.
pub fn read_meta_from_jar(jar: &Path) -> Result<PluginMeta, String> {
    let file = File::open(jar).map_err(|err| format!("cannot open: {err}"))?;
    let mut archive = ZipArchive::new(file).map_err(|err| format!("not a valid jar: {err}"))?;
    let Some(text) = read_entry(&mut archive, "plugin.yml")? else {
        if archive.by_name("paper-plugin.yml").is_ok() {
            return Err("paper-plugin.yml plugins are not supported yet".to_owned());
        }
        return Err("missing plugin.yml".to_owned());
    };
    parse_plugin_yml(&text).map_err(|err| err.to_string())
}

/// Scans `dir` for `*.jar` files, in file-name order.
///
/// A missing directory yields an empty result. Plugins whose names collide
/// (ignoring case) are reported as problems after the first.
pub fn discover(dir: &Path) -> io::Result<Discovery> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Discovery::default()),
        Err(err) => return Err(err),
    };
    let mut jars: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
        })
        .collect();
    jars.sort();

    let mut discovery = Discovery::default();
    let mut seen = HashSet::new();
    for jar in jars {
        match read_meta_from_jar(&jar) {
            Ok(meta) => {
                if seen.insert(meta.name.to_ascii_lowercase()) {
                    discovery.plugins.push(DiscoveredPlugin { jar, meta });
                } else {
                    let reason = format!("duplicate plugin name {:?}", meta.name);
                    discovery.problems.push(DiscoveryProblem { jar, reason });
                }
            }
            Err(reason) => discovery.problems.push(DiscoveryProblem { jar, reason }),
        }
    }
    Ok(discovery)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn write_jar(path: &Path, entries: &[(&str, &str)]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn yml(name: &str) -> String {
        format!("name: {name}\nmain: com.example.{name}\nversion: 1.0\n")
    }

    #[test]
    fn discovers_valid_jars_in_file_name_order() {
        let dir = tempfile::tempdir().unwrap();
        write_jar(&dir.path().join("b.jar"), &[("plugin.yml", &yml("Beta"))]);
        write_jar(&dir.path().join("a.JAR"), &[("plugin.yml", &yml("Alpha"))]);
        let found = discover(dir.path()).unwrap();
        let names: Vec<&str> = found.plugins.iter().map(|p| p.meta.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta"]);
        assert!(found.problems.is_empty());
    }

    #[test]
    fn ignores_non_jars_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        fs::create_dir(dir.path().join("folder.jar")).unwrap();
        let found = discover(dir.path()).unwrap();
        assert_eq!(found, Discovery::default());
    }

    #[test]
    fn a_missing_directory_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let found = discover(&dir.path().join("nope")).unwrap();
        assert_eq!(found, Discovery::default());
    }

    #[test]
    fn reports_unusable_jars_without_stopping() {
        let dir = tempfile::tempdir().unwrap();
        write_jar(
            &dir.path().join("good.jar"),
            &[("plugin.yml", &yml("Good"))],
        );
        fs::write(dir.path().join("broken.jar"), "not a zip").unwrap();
        write_jar(&dir.path().join("empty.jar"), &[("readme.txt", "x")]);
        write_jar(
            &dir.path().join("paper.jar"),
            &[("paper-plugin.yml", "name: P\n")],
        );
        write_jar(
            &dir.path().join("badyml.jar"),
            &[("plugin.yml", "name: [\n")],
        );
        let found = discover(dir.path()).unwrap();
        assert_eq!(found.plugins.len(), 1);
        assert_eq!(found.problems.len(), 4);
        let reason = |file: &str| {
            let jar = dir.path().join(file);
            found
                .problems
                .iter()
                .find(|p| p.jar == jar)
                .unwrap()
                .reason
                .clone()
        };
        assert!(reason("broken.jar").contains("not a valid jar"));
        assert!(reason("empty.jar").contains("missing plugin.yml"));
        assert!(reason("paper.jar").contains("paper-plugin.yml"));
        assert!(reason("badyml.jar").contains("not valid YAML"));
    }

    #[test]
    fn oversized_metadata_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let huge = format!("{}x: {}\n", yml("Big"), "a".repeat(MAX_META_BYTES));
        write_jar(&dir.path().join("big.jar"), &[("plugin.yml", &huge)]);
        let found = discover(dir.path()).unwrap();
        assert!(found.plugins.is_empty());
        assert!(found.problems[0].reason.contains("too large"));
    }

    #[test]
    fn duplicate_names_are_reported_after_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_jar(&dir.path().join("one.jar"), &[("plugin.yml", &yml("Same"))]);
        write_jar(&dir.path().join("two.jar"), &[("plugin.yml", &yml("same"))]);
        let found = discover(dir.path()).unwrap();
        assert_eq!(found.plugins.len(), 1);
        assert_eq!(found.plugins[0].jar, dir.path().join("one.jar"));
        assert!(found.problems[0].reason.contains("duplicate"));
    }
}
