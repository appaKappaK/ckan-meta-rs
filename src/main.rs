use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use flate2::read::GzDecoder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::Archive;
use zip::ZipArchive;

#[derive(Debug, Parser)]
#[command(name = "ckan-meta-rs")]
#[command(about = "Experimental CKAN metadata archive parser and benchmark tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse a CKAN metadata archive and report timing information.
    Parse {
        /// Path to a CKAN metadata .zip or .tar.gz archive.
        archive: PathBuf,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,

        /// Number of parse errors to show in the terminal report.
        #[arg(long, default_value_t = 8)]
        max_errors: usize,
    },

    /// Parse the same archive repeatedly and report timing statistics.
    Bench {
        /// Path to a CKAN metadata .zip or .tar.gz archive.
        archive: PathBuf,

        /// Number of measured runs.
        #[arg(long, default_value_t = 10)]
        runs: usize,

        /// Number of warmup runs to discard before measuring.
        #[arg(long, default_value_t = 2)]
        warmups: usize,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone)]
struct TextEntry {
    path: String,
    contents: String,
}

#[derive(Debug, Clone, Serialize)]
struct ParseReport {
    archive: String,
    archive_kind: String,
    archive_entries: usize,
    relevant_entries: usize,
    ckan_entries: usize,
    parsed_modules: usize,
    named_modules: usize,
    versioned_modules: usize,
    spec_versioned_modules: usize,
    unique_identifiers: usize,
    duplicate_identifiers: usize,
    missing_identifier: usize,
    dependency_edges: usize,
    recommendation_edges: usize,
    suggestion_edges: usize,
    conflict_edges: usize,
    provided_identifiers: usize,
    parse_errors: usize,
    download_counts: Option<usize>,
    builds: Option<usize>,
    repositories: Option<usize>,
    bytes_read: u64,
    read_ms: u128,
    parse_ms: u128,
    elapsed_ms: u128,
    top_identifiers: Vec<IdentifierCount>,
    errors: Vec<ParseError>,
}

#[derive(Debug, Serialize)]
struct BenchReport {
    archive: String,
    archive_kind: String,
    warmups: usize,
    runs: usize,
    sample: ParseReport,
    read_ms: TimingStats,
    parse_ms: TimingStats,
    elapsed_ms: TimingStats,
}

#[derive(Debug, Serialize)]
struct TimingStats {
    min: u128,
    max: u128,
    avg: f64,
    total: u128,
    values: Vec<u128>,
}

impl TimingStats {
    fn from_values(values: impl Iterator<Item = u128>) -> Self {
        let values = values.collect::<Vec<_>>();
        let total = values.iter().sum::<u128>();
        let min = values.iter().copied().min().unwrap_or(0);
        let max = values.iter().copied().max().unwrap_or(0);
        let avg = if values.is_empty() {
            0.0
        } else {
            total as f64 / values.len() as f64
        };

        Self {
            min,
            max,
            avg,
            total,
            values,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct IdentifierCount {
    identifier: String,
    versions: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ParseError {
    path: String,
    error: String,
}

#[derive(Debug)]
struct ArchiveLoad {
    archive_entries: usize,
    entries: Vec<TextEntry>,
    bytes_read: u64,
    elapsed: Duration,
}

#[derive(Debug, Deserialize)]
struct MinimalModule {
    identifier: Option<String>,
    name: Option<String>,
    version: Option<Value>,
    spec_version: Option<Value>,
    depends: Option<Value>,
    recommends: Option<Value>,
    suggests: Option<Value>,
    conflicts: Option<Value>,
    provides: Option<Value>,
}

#[derive(Debug)]
struct ParsedModule {
    identifier: Option<String>,
    name: Option<String>,
    version: Option<String>,
    spec_version: Option<String>,
    dependency_edges: usize,
    recommendation_edges: usize,
    suggestion_edges: usize,
    conflict_edges: usize,
    provided_identifiers: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Parse {
            archive,
            json,
            max_errors,
        } => parse_archive(archive, json, max_errors),
        Command::Bench {
            archive,
            runs,
            warmups,
            json,
        } => bench_archive(archive, runs, warmups, json),
    }
}

fn parse_archive(archive: PathBuf, json: bool, max_errors: usize) -> Result<()> {
    let report = parse_archive_report(archive)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report, max_errors);
    }

    Ok(())
}

fn bench_archive(archive: PathBuf, runs: usize, warmups: usize, json: bool) -> Result<()> {
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
    let report = BenchReport {
        archive: sample.archive.clone(),
        archive_kind: sample.archive_kind.clone(),
        warmups,
        runs,
        read_ms: TimingStats::from_values(reports.iter().map(|report| report.read_ms)),
        parse_ms: TimingStats::from_values(reports.iter().map(|report| report.parse_ms)),
        elapsed_ms: TimingStats::from_values(reports.iter().map(|report| report.elapsed_ms)),
        sample,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_bench_report(&report);
    }

    Ok(())
}

fn parse_archive_report(archive: PathBuf) -> Result<ParseReport> {
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

    let special = parse_special_entries(&loaded.entries);
    let identifier_counts = identifier_counts(&modules);
    let missing_identifier = modules
        .iter()
        .filter(|module| module.identifier.as_deref().unwrap_or("").is_empty())
        .count();
    let dependency_edges = sum_module_count(&modules, |module| module.dependency_edges);
    let recommendation_edges = sum_module_count(&modules, |module| module.recommendation_edges);
    let suggestion_edges = sum_module_count(&modules, |module| module.suggestion_edges);
    let conflict_edges = sum_module_count(&modules, |module| module.conflict_edges);
    let provided_identifiers = sum_module_count(&modules, |module| module.provided_identifiers);
    let top_identifiers = identifier_counts
        .iter()
        .rev()
        .take(10)
        .map(|(versions, identifier)| IdentifierCount {
            identifier: identifier.clone(),
            versions: *versions,
        })
        .collect::<Vec<_>>();

    Ok(ParseReport {
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
        dependency_edges,
        recommendation_edges,
        suggestion_edges,
        conflict_edges,
        provided_identifiers,
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
    })
}

fn archive_kind(path: &Path) -> Result<&'static str> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if file_name.ends_with(".zip") {
        Ok("zip")
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        Ok("tar.gz")
    } else {
        bail!("unsupported archive type: {}", path.display());
    }
}

fn load_archive(path: &Path, kind: &str) -> Result<ArchiveLoad> {
    match kind {
        "zip" => load_zip(path),
        "tar.gz" => load_tar_gz(path),
        _ => bail!("unsupported archive type: {kind}"),
    }
}

fn load_zip(path: &Path) -> Result<ArchiveLoad> {
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive {}", path.display()))?;
    let archive_entries = archive.len();
    let mut entries = Vec::new();
    let mut bytes_read = 0;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_string();
        if !is_relevant_entry(&name) {
            continue;
        }

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .with_context(|| format!("failed to read {name} as UTF-8 text"))?;
        bytes_read += file.size();
        entries.push(TextEntry {
            path: name,
            contents,
        });
    }

    Ok(ArchiveLoad {
        archive_entries,
        entries,
        bytes_read,
        elapsed: started.elapsed(),
    })
}

fn load_tar_gz(path: &Path) -> Result<ArchiveLoad> {
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut archive_entries = 0;
    let mut entries = Vec::new();
    let mut bytes_read = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        archive_entries += 1;
        let path = entry.path()?.to_string_lossy().to_string();
        if !is_relevant_entry(&path) {
            continue;
        }

        let size = entry.size();
        let mut contents = String::new();
        entry
            .read_to_string(&mut contents)
            .with_context(|| format!("failed to read {path} as UTF-8 text"))?;
        bytes_read += size;
        entries.push(TextEntry { path, contents });
    }

    Ok(ArchiveLoad {
        archive_entries,
        entries,
        bytes_read,
        elapsed: started.elapsed(),
    })
}

fn is_relevant_entry(path: &str) -> bool {
    path.ends_with(".ckan")
        || path.ends_with("download_counts.json")
        || path.ends_with("builds.json")
        || path.ends_with("repositories.json")
}

fn parse_module_entry(entry: &&TextEntry) -> Result<ParsedModule, ParseError> {
    let raw =
        serde_json::from_str::<MinimalModule>(&entry.contents).map_err(|error| ParseError {
            path: entry.path.clone(),
            error: error.to_string(),
        })?;

    Ok(ParsedModule {
        identifier: raw.identifier.map(clean_string),
        name: raw.name.map(clean_string),
        version: raw.version.as_ref().map(value_to_text),
        spec_version: raw.spec_version.as_ref().map(value_to_text),
        dependency_edges: collection_len(raw.depends.as_ref()),
        recommendation_edges: collection_len(raw.recommends.as_ref()),
        suggestion_edges: collection_len(raw.suggests.as_ref()),
        conflict_edges: collection_len(raw.conflicts.as_ref()),
        provided_identifiers: collection_len(raw.provides.as_ref()),
    })
}

fn clean_string(value: String) -> String {
    value.trim().to_string()
}

fn has_text(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.is_empty())
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn collection_len(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(items)) => items.len(),
        Some(Value::Null) | None => 0,
        Some(_) => 1,
    }
}

fn sum_module_count(modules: &[ParsedModule], count: impl Fn(&ParsedModule) -> usize) -> usize {
    modules.iter().map(count).sum()
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

fn print_report(report: &ParseReport, max_errors: usize) {
    println!("Archive: {}", report.archive);
    println!("Type: {}", report.archive_kind);
    println!("Archive entries: {}", report.archive_entries);
    println!("Relevant entries: {}", report.relevant_entries);
    println!("CKAN metadata entries: {}", report.ckan_entries);
    println!("Parsed modules: {}", report.parsed_modules);
    println!("Named modules: {}", report.named_modules);
    println!("Versioned modules: {}", report.versioned_modules);
    println!("Spec-versioned modules: {}", report.spec_versioned_modules);
    println!("Unique identifiers: {}", report.unique_identifiers);
    println!("Duplicate identifiers: {}", report.duplicate_identifiers);
    println!("Missing identifiers: {}", report.missing_identifier);
    println!(
        "Relationship edges: depends={} recommends={} suggests={} conflicts={} provides={}",
        report.dependency_edges,
        report.recommendation_edges,
        report.suggestion_edges,
        report.conflict_edges,
        report.provided_identifiers
    );
    println!("Parse errors: {}", report.parse_errors);
    println!(
        "Special files: download_counts={} builds={} repositories={}",
        option_count(report.download_counts),
        option_count(report.builds),
        option_count(report.repositories)
    );
    println!("Bytes read: {}", report.bytes_read);
    println!(
        "Timing: read={}ms parse={}ms total={}ms",
        report.read_ms, report.parse_ms, report.elapsed_ms
    );

    if !report.top_identifiers.is_empty() {
        println!();
        println!("Top identifiers by version count:");
        for item in &report.top_identifiers {
            println!("  {:>4} {}", item.versions, item.identifier);
        }
    }

    if !report.errors.is_empty() && max_errors > 0 {
        println!();
        println!("First parse errors:");
        for error in report.errors.iter().take(max_errors) {
            println!("  {}: {}", error.path, error.error);
        }
    }
}

fn print_bench_report(report: &BenchReport) {
    println!("Archive: {}", report.archive);
    println!("Type: {}", report.archive_kind);
    println!("Warmups: {}", report.warmups);
    println!("Runs: {}", report.runs);
    println!("Parsed modules: {}", report.sample.parsed_modules);
    println!("Unique identifiers: {}", report.sample.unique_identifiers);
    println!(
        "Relationship edges: depends={} recommends={} suggests={} conflicts={} provides={}",
        report.sample.dependency_edges,
        report.sample.recommendation_edges,
        report.sample.suggestion_edges,
        report.sample.conflict_edges,
        report.sample.provided_identifiers
    );
    println!("Parse errors: {}", report.sample.parse_errors);
    println!("Bytes read per run: {}", report.sample.bytes_read);
    println!();
    println!("Timing statistics:");
    print_timing_stats("read", &report.read_ms);
    print_timing_stats("parse", &report.parse_ms);
    print_timing_stats("total", &report.elapsed_ms);
}

fn print_timing_stats(label: &str, stats: &TimingStats) {
    println!(
        "  {:<5} min={}ms avg={:.2}ms max={}ms total={}ms",
        label, stats.min, stats.avg, stats.max, stats.total
    );
}

fn option_count(value: Option<usize>) -> String {
    value
        .map(|count| count.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_module() {
        let entry = TextEntry {
            path: "Example.ckan".to_string(),
            contents: r#"{
                "spec_version": 1,
                "identifier": "Example",
                "name": "Example Mod",
                "version": "1.2.3",
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
