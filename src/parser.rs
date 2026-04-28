use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::archive::{archive_kind, load_archive, TextEntry};
use crate::model::{
    clean_string, collection_len, has_text, has_value, value_to_text, BenchReport, IdentifierCount,
    MinimalModule, ModuleSummary, ParseError, ParseReport, ParsedModule, TimingStats,
};

#[derive(Debug)]
pub struct ArchiveParse {
    pub report: ParseReport,
    pub modules: Vec<ParsedModule>,
}

pub fn parse_archive_report(archive: PathBuf) -> Result<ParseReport> {
    Ok(parse_archive_details(archive)?.report)
}

pub fn parse_archive_details(archive: PathBuf) -> Result<ArchiveParse> {
    if !archive.is_file() {
        bail!(
            "archive does not exist or is not a file: {}",
            archive.display()
        );
    }

    let started = Instant::now();
    let archive_kind = archive_kind(&archive)?;
    let loaded = load_archive(&archive, archive_kind)?;
    let parse_started = Instant::now();

    let ckan_entries: Vec<&TextEntry> = loaded
        .entries
        .iter()
        .filter(|entry| entry.path.ends_with(".ckan"))
        .collect();

    let parsed: Vec<Result<ParsedModule, ParseError>> = ckan_entries
        .par_iter()
        .map(|entry| parse_module_entry(entry))
        .collect();

    let mut modules = Vec::new();
    let mut errors = Vec::new();
    for item in parsed {
        match item {
            Ok(module) => modules.push(module),
            Err(error) => errors.push(error),
        }
    }

    modules.sort_by(|left, right| {
        left.identifier
            .cmp(&right.identifier)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.path.cmp(&right.path))
    });

    let special = parse_special_entries(&loaded.entries);
    let identifier_counts = identifier_counts(&modules);
    let missing_identifier = modules
        .iter()
        .filter(|module| module.identifier.as_deref().unwrap_or("").is_empty())
        .count();
    let top_identifiers = identifier_counts
        .iter()
        .rev()
        .take(10)
        .map(|(versions, identifier)| IdentifierCount {
            identifier: identifier.clone(),
            versions: *versions,
        })
        .collect::<Vec<_>>();

    let report = ParseReport {
        archive: archive.display().to_string(),
        archive_kind: archive_kind.to_string(),
        archive_entries: loaded.archive_entries,
        relevant_entries: loaded.entries.len(),
        ckan_entries: ckan_entries.len(),
        parsed_modules: modules.len(),
        named_modules: modules
            .iter()
            .filter(|module| has_text(&module.name))
            .count(),
        versioned_modules: modules
            .iter()
            .filter(|module| has_text(&module.version))
            .count(),
        spec_versioned_modules: modules
            .iter()
            .filter(|module| has_text(&module.spec_version))
            .count(),
        unique_identifiers: modules
            .iter()
            .filter_map(|module| module.identifier.as_ref())
            .collect::<BTreeSet<_>>()
            .len(),
        duplicate_identifiers: identifier_counts
            .iter()
            .filter(|(versions, _)| *versions > 1)
            .count(),
        missing_identifier,
        dependency_edges: sum_module_count(&modules, |module| module.dependency_edges),
        recommendation_edges: sum_module_count(&modules, |module| module.recommendation_edges),
        suggestion_edges: sum_module_count(&modules, |module| module.suggestion_edges),
        conflict_edges: sum_module_count(&modules, |module| module.conflict_edges),
        provided_identifiers: sum_module_count(&modules, |module| module.provided_identifiers),
        modules_with_abstract: modules
            .iter()
            .filter(|module| has_text(&module.abstract_text))
            .count(),
        modules_with_author: modules
            .iter()
            .filter(|module| module.author_count > 0)
            .count(),
        modules_with_license: modules
            .iter()
            .filter(|module| module.license_count > 0)
            .count(),
        modules_with_install: modules
            .iter()
            .filter(|module| module.install_steps > 0)
            .count(),
        modules_with_resources: modules
            .iter()
            .filter(|module| module.resource_count > 0)
            .count(),
        modules_with_download: modules.iter().filter(|module| module.has_download).count(),
        modules_with_ksp_version: modules
            .iter()
            .filter(|module| has_text(&module.ksp_version))
            .count(),
        modules_with_ksp_version_min: modules
            .iter()
            .filter(|module| has_text(&module.ksp_version_min))
            .count(),
        modules_with_ksp_version_max: modules
            .iter()
            .filter(|module| has_text(&module.ksp_version_max))
            .count(),
        parse_errors: errors.len(),
        download_counts: special.download_counts,
        builds: special.builds,
        repositories: special.repositories,
        bytes_read: loaded.bytes_read,
        read_ms: loaded.elapsed.as_millis(),
        parse_ms: parse_started.elapsed().as_millis(),
        elapsed_ms: started.elapsed().as_millis(),
        top_identifiers,
        errors,
    };

    Ok(ArchiveParse { report, modules })
}

pub fn benchmark_archive(archive: PathBuf, runs: usize, warmups: usize) -> Result<BenchReport> {
    if runs == 0 {
        bail!("runs must be greater than zero");
    }

    for _ in 0..warmups {
        parse_archive_report(archive.clone())?;
    }

    let mut reports = Vec::with_capacity(runs);
    for _ in 0..runs {
        reports.push(parse_archive_report(archive.clone())?);
    }

    let sample = reports
        .first()
        .cloned()
        .context("benchmark produced no samples")?;

    Ok(BenchReport {
        archive: sample.archive.clone(),
        archive_kind: sample.archive_kind.clone(),
        warmups,
        runs,
        read_ms: TimingStats::from_values(reports.iter().map(|report| report.read_ms)),
        parse_ms: TimingStats::from_values(reports.iter().map(|report| report.parse_ms)),
        elapsed_ms: TimingStats::from_values(reports.iter().map(|report| report.elapsed_ms)),
        sample,
    })
}

pub fn module_summaries(archive: PathBuf, limit: Option<usize>) -> Result<Vec<ModuleSummary>> {
    let parsed = parse_archive_details(archive)?;
    let mut modules = parsed.modules;
    if let Some(limit) = limit {
        modules.truncate(limit);
    }

    Ok(modules.iter().map(ModuleSummary::from).collect::<Vec<_>>())
}

fn parse_module_entry(entry: &&TextEntry) -> Result<ParsedModule, ParseError> {
    let raw =
        serde_json::from_str::<MinimalModule>(&entry.contents).map_err(|error| ParseError {
            path: entry.path.clone(),
            error: error.to_string(),
        })?;

    Ok(ParsedModule {
        path: entry.path.clone(),
        identifier: raw.identifier.map(clean_string),
        name: raw.name.map(clean_string),
        version: raw.version.as_ref().map(value_to_text),
        spec_version: raw.spec_version.as_ref().map(value_to_text),
        abstract_text: raw.abstract_text.map(clean_string),
        author_count: collection_len(raw.author.as_ref()),
        license_count: collection_len(raw.license.as_ref()),
        resource_count: collection_len(raw.resources.as_ref()),
        install_steps: collection_len(raw.install.as_ref()),
        has_download: has_value(raw.download.as_ref()),
        ksp_version: raw.ksp_version.as_ref().map(value_to_text),
        ksp_version_min: raw.ksp_version_min.as_ref().map(value_to_text),
        ksp_version_max: raw.ksp_version_max.as_ref().map(value_to_text),
        dependency_edges: collection_len(raw.depends.as_ref()),
        recommendation_edges: collection_len(raw.recommends.as_ref()),
        suggestion_edges: collection_len(raw.suggests.as_ref()),
        conflict_edges: collection_len(raw.conflicts.as_ref()),
        provided_identifiers: collection_len(raw.provides.as_ref()),
    })
}

#[derive(Debug, Default)]
struct SpecialCounts {
    download_counts: Option<usize>,
    builds: Option<usize>,
    repositories: Option<usize>,
}

fn parse_special_entries(entries: &[TextEntry]) -> SpecialCounts {
    let mut counts = SpecialCounts::default();

    for entry in entries {
        if entry.path.ends_with("download_counts.json") {
            counts.download_counts = count_json_object(&entry.contents);
        } else if entry.path.ends_with("builds.json") {
            counts.builds = count_json_named_array_or_object(&entry.contents, "builds");
        } else if entry.path.ends_with("repositories.json") {
            counts.repositories = count_json_named_array_or_object(&entry.contents, "repositories");
        }
    }

    counts
}

fn count_json_object(contents: &str) -> Option<usize> {
    serde_json::from_str::<Value>(contents)
        .ok()?
        .as_object()
        .map(|obj| obj.len())
}

fn count_json_named_array_or_object(contents: &str, key: &str) -> Option<usize> {
    let value = serde_json::from_str::<Value>(contents).ok()?;
    let nested = value.get(key).unwrap_or(&value);

    nested
        .as_array()
        .map(|array| array.len())
        .or_else(|| nested.as_object().map(|obj| obj.len()))
}

fn identifier_counts(modules: &[ParsedModule]) -> Vec<(usize, String)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for module in modules {
        if let Some(identifier) = module.identifier.as_ref() {
            if !identifier.is_empty() {
                *counts.entry(identifier.clone()).or_default() += 1;
            }
        }
    }

    let mut counts = counts
        .into_iter()
        .map(|(identifier, versions)| (versions, identifier))
        .collect::<Vec<_>>();
    counts.sort();
    counts
}

fn sum_module_count(modules: &[ParsedModule], count: impl Fn(&ParsedModule) -> usize) -> usize {
    modules.iter().map(count).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::is_relevant_entry;

    #[test]
    fn parses_minimal_module() {
        let entry = TextEntry {
            path: "Example.ckan".to_string(),
            contents: r#"{
                "spec_version": 1,
                "identifier": "Example",
                "name": "Example Mod",
                "abstract": "Short description",
                "author": ["First", "Second"],
                "license": "MIT",
                "version": "1.2.3",
                "ksp_version_min": "1.12",
                "ksp_version_max": "1.12.5",
                "download": "https://example.invalid/mod.zip",
                "install": [
                    { "find": "GameData", "install_to": "GameData" }
                ],
                "resources": {
                    "homepage": "https://example.invalid"
                },
                "depends": [
                    { "name": "ModuleManager" },
                    { "name": "Harmony" }
                ],
                "recommends": [
                    { "name": "ToolbarController" }
                ],
                "suggests": [],
                "conflicts": [
                    { "name": "OldExample" }
                ],
                "provides": [ "ExampleVirtual" ]
            }"#
            .to_string(),
        };

        let parsed = parse_module_entry(&&entry).expect("module should parse");

        assert_eq!(parsed.identifier.as_deref(), Some("Example"));
        assert_eq!(parsed.name.as_deref(), Some("Example Mod"));
        assert_eq!(parsed.version.as_deref(), Some("1.2.3"));
        assert_eq!(parsed.spec_version.as_deref(), Some("1"));
        assert_eq!(parsed.abstract_text.as_deref(), Some("Short description"));
        assert_eq!(parsed.author_count, 2);
        assert_eq!(parsed.license_count, 1);
        assert_eq!(parsed.install_steps, 1);
        assert_eq!(parsed.resource_count, 1);
        assert!(parsed.has_download);
        assert_eq!(parsed.dependency_edges, 2);
        assert_eq!(parsed.recommendation_edges, 1);
        assert_eq!(parsed.suggestion_edges, 0);
        assert_eq!(parsed.conflict_edges, 1);
        assert_eq!(parsed.provided_identifiers, 1);
    }

    #[test]
    fn identifies_relevant_entries() {
        assert!(is_relevant_entry("ModuleManager.ckan"));
        assert!(is_relevant_entry("CKAN-meta/download_counts.json"));
        assert!(is_relevant_entry("CKAN-meta/builds.json"));
        assert!(is_relevant_entry("CKAN-meta/repositories.json"));
        assert!(!is_relevant_entry("README.md"));
    }
}
