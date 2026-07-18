use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::archive::{ArchiveLoad, TextEntry};

#[derive(Debug, Deserialize)]
struct RepositoryCache {
    #[serde(default)]
    available_modules: BTreeMap<String, CachedAvailableModule>,
    #[serde(default)]
    download_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct CachedAvailableModule {
    #[serde(default)]
    module_version: BTreeMap<String, Value>,
}

/// Load the parsed JSON files written by CKAN's RepositoryDataManager.
///
/// Sources must be supplied in CKAN priority order. If two repositories contain
/// the same identifier and version, the first source wins, matching CKAN's
/// AvailableModule merge behavior.
pub fn load_repository_caches(paths: &[PathBuf]) -> Result<ArchiveLoad> {
    if paths.is_empty() {
        bail!("at least one CKAN repository cache is required");
    }

    let started = Instant::now();
    let mut archive_entries = 0;
    let mut bytes_read = 0;
    let mut modules = BTreeMap::<(String, String), Value>::new();
    let mut download_counts = BTreeMap::<String, u64>::new();

    for path in paths {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read CKAN repository cache {}", path.display()))?;
        bytes_read += bytes.len() as u64;
        let cache = serde_json::from_slice::<RepositoryCache>(&bytes)
            .with_context(|| format!("failed to parse CKAN repository cache {}", path.display()))?;

        for (outer_identifier, available) in cache.available_modules {
            for (outer_version, module) in available.module_version {
                archive_entries += 1;
                let identifier = module
                    .get("identifier")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&outer_identifier)
                    .to_string();
                let version = module
                    .get("version")
                    .map(value_to_key)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(outer_version);

                modules.entry((identifier, version)).or_insert(module);
            }
        }

        for (identifier, count) in cache.download_counts {
            download_counts.entry(identifier).or_insert(count);
        }
    }

    let mut entries = modules
        .into_iter()
        .enumerate()
        .map(|(index, ((identifier, version), module))| {
            Ok(TextEntry {
                path: format!(
                    "repository-cache/{index:08}-{}-{}.ckan",
                    safe_label(&identifier),
                    safe_label(&version)
                ),
                contents: serde_json::to_string(&module)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    if !download_counts.is_empty() {
        entries.push(TextEntry {
            path: "repository-cache/download_counts.json".to_string(),
            contents: serde_json::to_string(&download_counts)?,
        });
    }

    Ok(ArchiveLoad {
        archive_entries,
        entries,
        bytes_read,
        elapsed: started.elapsed(),
    })
}

fn value_to_key(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn safe_label(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn merges_ordered_caches_with_first_repository_winning() {
        let root = unique_temp_dir();
        let first = root.join("first.json");
        let second = root.join("second.json");
        fs::write(
            &first,
            r#"{
                "available_modules": {
                    "Example": { "module_version": {
                        "1.0": { "identifier": "Example", "name": "Preferred", "version": "1.0" }
                    }}
                },
                "download_counts": { "Example": 42 }
            }"#,
        )
        .unwrap();
        fs::write(
            &second,
            r#"{
                "available_modules": {
                    "Example": { "module_version": {
                        "1.0": { "identifier": "Example", "name": "Lower priority", "version": "1.0" },
                        "2.0": { "identifier": "Example", "name": "New version", "version": "2.0" }
                    }}
                },
                "download_counts": { "Example": 7 }
            }"#,
        )
        .unwrap();

        let loaded = load_repository_caches(&[first, second]).unwrap();
        let module_entries = loaded
            .entries
            .iter()
            .filter(|entry| entry.path.ends_with(".ckan"))
            .collect::<Vec<_>>();
        let preferred = module_entries
            .iter()
            .map(|entry| serde_json::from_str::<Value>(&entry.contents).unwrap())
            .find(|module| module["version"] == "1.0")
            .unwrap();
        let counts = loaded
            .entries
            .iter()
            .find(|entry| entry.path.ends_with("download_counts.json"))
            .unwrap();

        assert_eq!(module_entries.len(), 2);
        assert_eq!(preferred["name"], "Preferred");
        assert_eq!(
            serde_json::from_str::<Value>(&counts.contents).unwrap()["Example"],
            42
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ckan-meta-rs-cache-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
