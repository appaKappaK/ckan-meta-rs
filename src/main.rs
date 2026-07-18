use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod archive;
mod download;
mod export_file;
mod model;
mod output;
mod parser;
mod repository_cache;

use archive::extract_relevant_entries;
use download::download_to_file;
use export_file::{validate_catalog_index_file, validate_export_file};
use model::SyncReport;
use output::{
    print_bench_report, print_catalog_index_validation, print_compare_report,
    print_download_report, print_export_validation, print_extraction_report,
    print_module_inspection, print_module_summaries, print_relation_matches, print_relation_stats,
    print_report, print_sync_report, print_unresolved_relations, write_catalog_index,
    write_export_package,
};
use parser::{
    benchmark_archive, catalog_index, catalog_index_from_repository_caches, compare_archives,
    export_package, find_module_summaries, inspect_module, latest_module_summaries,
    module_summaries, parse_archive_report, relation_matches, relation_stats, unresolved_relations,
};

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
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
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
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
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

    /// Emit per-module summaries for compatibility comparison work.
    Modules {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
        archive: PathBuf,

        /// Emit one JSON object per line.
        #[arg(long)]
        json_lines: bool,

        /// Emit one pretty-printed JSON array.
        #[arg(long)]
        json: bool,

        /// Limit the number of module rows emitted.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Emit the latest module summary for each identifier.
    Latest {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
        archive: PathBuf,

        /// Emit one JSON object per line.
        #[arg(long)]
        json_lines: bool,

        /// Emit one pretty-printed JSON array.
        #[arg(long)]
        json: bool,

        /// Limit the number of module rows emitted.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Export a stable module summary file.
    Export {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Output file path, or '-' for stdout.
        #[arg(short, long)]
        output: PathBuf,

        /// Emit one module JSON object per line instead of a package object.
        #[arg(long)]
        json_lines: bool,
    },

    /// Export a CKAN-Linux catalog/search sidecar index.
    CatalogIndex {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Output file path, or '-' for stdout.
        #[arg(short, long)]
        output: PathBuf,

        /// Include only the latest version per identifier.
        #[arg(long)]
        latest_only: bool,

        /// Pretty-print JSON instead of compact JSON.
        #[arg(long)]
        pretty: bool,
    },

    /// Validate a CKAN-Linux catalog/search sidecar index.
    ValidateCatalogIndex {
        /// Catalog index file path.
        input: PathBuf,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Build and atomically replace a CKAN-Linux sidecar from CKAN's own cache.
    RefreshSidecar {
        /// Active CKAN repository cache JSON file, in priority order. Repeat for each repository.
        #[arg(long = "repository-cache", required = true)]
        repository_caches: Vec<PathBuf>,

        /// Sidecar file to replace after generation and validation succeeds.
        #[arg(short, long)]
        output: PathBuf,

        /// Fingerprint supplied by CKAN for the ordered repository snapshot.
        #[arg(long)]
        source_fingerprint: Option<String>,

        /// Pretty-print the sidecar instead of writing compact JSON.
        #[arg(long)]
        pretty: bool,

        /// Emit a machine-readable completion report.
        #[arg(long)]
        json: bool,
    },

    /// Validate an exported summary file.
    ValidateExport {
        /// Export file path.
        input: PathBuf,

        /// Read the input as JSON lines instead of package JSON.
        #[arg(long)]
        json_lines: bool,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Download a CKAN metadata archive.
    Fetch {
        /// URL to download.
        #[arg(
            long,
            default_value = "https://github.com/KSP-CKAN/CKAN-meta/archive/refs/heads/master.zip"
        )]
        url: String,

        /// Output archive path.
        #[arg(short, long, default_value = "data/CKAN-meta-master.zip")]
        output: PathBuf,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Download, extract, and optionally export CKAN metadata.
    Sync {
        /// URL to download.
        #[arg(
            long,
            default_value = "https://github.com/KSP-CKAN/CKAN-meta/archive/refs/heads/master.zip"
        )]
        url: String,

        /// Output archive path.
        #[arg(long, default_value = "data/CKAN-meta-master.zip")]
        archive: PathBuf,

        /// Destination cache directory.
        #[arg(long, default_value = "data/CKAN-meta-cache")]
        cache_dir: PathBuf,

        /// Optional export file to write after cache extraction.
        #[arg(long)]
        export: Option<PathBuf>,

        /// Write export as JSON lines instead of a package object.
        #[arg(long)]
        json_lines: bool,

        /// Remove the cache directory before extracting.
        #[arg(long, default_value_t = true)]
        clean: bool,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Extract relevant metadata files into a persistent cache directory.
    Cache {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or source directory.
        archive: PathBuf,

        /// Destination cache directory.
        cache_dir: PathBuf,

        /// Remove the cache directory before extracting.
        #[arg(long)]
        clean: bool,

        /// Optional export file to write after cache extraction.
        #[arg(long)]
        export: Option<PathBuf>,

        /// Write export as JSON lines instead of a package object.
        #[arg(long)]
        json_lines: bool,

        /// Emit machine-readable extraction JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Search module summaries by identifier, name, or version.
    Find {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Case-insensitive query text.
        query: String,

        /// Emit one JSON object per line.
        #[arg(long)]
        json_lines: bool,

        /// Emit one pretty-printed JSON array.
        #[arg(long)]
        json: bool,

        /// Limit the number of module rows emitted.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Show modules that reference a relationship target.
    Relations {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Relationship target identifier, such as ModuleManager.
        target: String,

        /// Emit one JSON object per line.
        #[arg(long)]
        json_lines: bool,

        /// Emit one pretty-printed JSON array.
        #[arg(long)]
        json: bool,

        /// Limit the number of rows emitted.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Count the most common relationship targets.
    RelationStats {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Number of rows to show.
        #[arg(long, default_value_t = 30)]
        limit: usize,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Find relationship targets not present as identifiers or provided virtual packages.
    Unresolved {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Relationship to inspect: depends, recommends, suggests, conflicts, or provides.
        #[arg(long, default_value = "depends")]
        relationship: String,

        /// Number of rows to show.
        #[arg(long, default_value_t = 30)]
        limit: usize,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Inspect a module and reverse relationships to it.
    Inspect {
        /// Path to a CKAN metadata source.
        archive: PathBuf,

        /// Exact module identifier to inspect.
        identifier: String,

        /// Exact version to inspect.
        #[arg(long, conflicts_with = "latest")]
        version: Option<String>,

        /// Inspect only the latest version by CKAN-ish version ordering.
        #[arg(long)]
        latest: bool,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,

        /// Limit the number of matched module rows.
        #[arg(long)]
        limit: Option<usize>,

        /// Limit the number of reverse relationship rows.
        #[arg(long)]
        reverse_limit: Option<usize>,
    },

    /// Compare metadata counts from two archives or directories.
    Compare {
        /// Left CKAN metadata source.
        left: PathBuf,

        /// Right CKAN metadata source.
        right: PathBuf,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
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
        } => bench(archive, runs, warmups, json),
        Command::Modules {
            archive,
            json_lines,
            json,
            limit,
        } => modules(archive, json, json_lines, limit),
        Command::Latest {
            archive,
            json_lines,
            json,
            limit,
        } => latest(archive, json, json_lines, limit),
        Command::Export {
            archive,
            output,
            json_lines,
        } => export(archive, output, json_lines),
        Command::CatalogIndex {
            archive,
            output,
            latest_only,
            pretty,
        } => catalog_index_command(archive, output, latest_only, pretty),
        Command::ValidateCatalogIndex { input, json } => validate_catalog_index(input, json),
        Command::RefreshSidecar {
            repository_caches,
            output,
            source_fingerprint,
            pretty,
            json,
        } => refresh_sidecar(repository_caches, output, source_fingerprint, pretty, json),
        Command::ValidateExport {
            input,
            json_lines,
            json,
        } => validate_export(input, json_lines, json),
        Command::Fetch { url, output, json } => fetch(url, output, json),
        Command::Sync {
            url,
            archive,
            cache_dir,
            export,
            json_lines,
            clean,
            json,
        } => sync(url, archive, cache_dir, export, json_lines, clean, json),
        Command::Cache {
            archive,
            cache_dir,
            clean,
            export,
            json_lines,
            json,
        } => cache(archive, cache_dir, clean, export, json_lines, json),
        Command::Find {
            archive,
            query,
            json_lines,
            json,
            limit,
        } => find(archive, query, json, json_lines, limit),
        Command::Relations {
            archive,
            target,
            json_lines,
            json,
            limit,
        } => relations(archive, target, json, json_lines, limit),
        Command::RelationStats {
            archive,
            limit,
            json,
        } => relation_target_stats(archive, limit, json),
        Command::Unresolved {
            archive,
            relationship,
            limit,
            json,
        } => unresolved(archive, relationship, limit, json),
        Command::Inspect {
            archive,
            identifier,
            version,
            latest,
            json,
            limit,
            reverse_limit,
        } => inspect(
            archive,
            identifier,
            version,
            latest,
            json,
            limit,
            reverse_limit,
        ),
        Command::Compare { left, right, json } => compare(left, right, json),
        Command::Completions { shell } => completions(shell),
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

fn bench(archive: PathBuf, runs: usize, warmups: usize, json: bool) -> Result<()> {
    let report = benchmark_archive(archive, runs, warmups)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_bench_report(&report);
    }

    Ok(())
}

fn modules(archive: PathBuf, json: bool, json_lines: bool, limit: Option<usize>) -> Result<()> {
    let modules = module_summaries(archive, limit)?;
    print_module_summaries(&modules, json, json_lines)
}

fn latest(archive: PathBuf, json: bool, json_lines: bool, limit: Option<usize>) -> Result<()> {
    let modules = latest_module_summaries(archive, limit)?;
    print_module_summaries(&modules, json, json_lines)
}

fn export(archive: PathBuf, output: PathBuf, json_lines: bool) -> Result<()> {
    let package = export_package(archive)?;
    write_export_package(&package, &output, json_lines)
}

fn catalog_index_command(
    archive: PathBuf,
    output: PathBuf,
    latest_only: bool,
    pretty: bool,
) -> Result<()> {
    let index = catalog_index(archive, latest_only)?;
    write_catalog_index(&index, &output, pretty)
}

fn validate_catalog_index(input: PathBuf, json: bool) -> Result<()> {
    let report = validate_catalog_index_file(&input)?;
    print_catalog_index_validation(&report, json)
}

fn refresh_sidecar(
    repository_caches: Vec<PathBuf>,
    output: PathBuf,
    source_fingerprint: Option<String>,
    pretty: bool,
    json: bool,
) -> Result<()> {
    let mut index =
        catalog_index_from_repository_caches(&repository_caches, true, source_fingerprint.clone())?;
    // Make the source stable and concise instead of embedding machine-specific
    // absolute cache paths in a public-facing index.
    index.source = format!(
        "CKAN repository cache ({} source(s))",
        repository_caches.len()
    );

    let temp = SiblingTempFile::create(&output)?;
    write_catalog_index(&index, temp.path(), pretty)?;
    let validation = validate_catalog_index_file(temp.path())?;
    temp.replace(&output)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "output": output.display().to_string(),
                "repository_caches": repository_caches.len(),
                "source_fingerprint": source_fingerprint,
                "schema_version": validation.schema_version,
                "modules": validation.modules,
                "unique_identifiers": validation.unique_identifiers,
                "latest_modules": validation.latest_modules,
            }))?
        );
    } else {
        println!(
            "Updated CKAN Linux catalog sidecar: {} ({} modules, {} identifiers)",
            output.display(),
            validation.modules,
            validation.unique_identifiers
        );
    }

    Ok(())
}

struct SiblingTempFile {
    path: PathBuf,
    keep: bool,
}

impl SiblingTempFile {
    fn create(output: &Path) -> Result<Self> {
        if output == Path::new("-") {
            anyhow::bail!("refresh-sidecar requires a file output path");
        }

        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let file_name = output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("catalog-index.json");

        for attempt in 0..1000_u32 {
            let candidate = parent.join(format!(
                ".{file_name}.{}.{}.tmp",
                std::process::id(),
                attempt
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    return Ok(Self {
                        path: candidate,
                        keep: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to create temporary sidecar beside {}",
                            output.display()
                        )
                    });
                }
            }
        }

        anyhow::bail!(
            "could not reserve a temporary sidecar beside {}",
            output.display()
        )
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn replace(mut self, output: &Path) -> Result<()> {
        fs::rename(&self.path, output).with_context(|| {
            format!(
                "failed to atomically replace {} with generated sidecar",
                output.display()
            )
        })?;
        self.keep = true;
        Ok(())
    }
}

impl Drop for SiblingTempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod sidecar_temp_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn abandoned_generation_preserves_existing_sidecar() {
        let root = unique_temp_dir();
        let output = root.join("catalog-index.json");
        fs::write(&output, "old").unwrap();

        {
            let temp = SiblingTempFile::create(&output).unwrap();
            fs::write(temp.path(), "invalid new file").unwrap();
        }

        assert_eq!(fs::read_to_string(&output).unwrap(), "old");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validated_generation_replaces_existing_sidecar() {
        let root = unique_temp_dir();
        let output = root.join("catalog-index.json");
        fs::write(&output, "old").unwrap();
        let temp = SiblingTempFile::create(&output).unwrap();
        fs::write(temp.path(), "new").unwrap();

        temp.replace(&output).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "new");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ckan-meta-rs-sidecar-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}

fn validate_export(input: PathBuf, json_lines: bool, json: bool) -> Result<()> {
    let report = validate_export_file(&input, json_lines)?;
    print_export_validation(&report, json)
}

fn fetch(url: String, output: PathBuf, json: bool) -> Result<()> {
    let report = download_to_file(&url, &output)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_download_report(&report);
    }

    Ok(())
}

fn sync(
    url: String,
    archive: PathBuf,
    cache_dir: PathBuf,
    export: Option<PathBuf>,
    json_lines: bool,
    clean: bool,
    json: bool,
) -> Result<()> {
    let download = download_to_file(&url, &archive)?;
    let extraction = extract_relevant_entries(&archive, &cache_dir, clean)?;
    let export_path = export.as_ref().map(|path| path.display().to_string());

    if let Some(output) = export {
        let package = export_package(cache_dir.clone())?;
        write_export_package(&package, &output, json_lines)?;
    }

    let report = SyncReport {
        download,
        extraction,
        export: export_path,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_sync_report(&report);
    }

    Ok(())
}

fn cache(
    archive: PathBuf,
    cache_dir: PathBuf,
    clean: bool,
    export: Option<PathBuf>,
    json_lines: bool,
    json: bool,
) -> Result<()> {
    let report = extract_relevant_entries(&archive, &cache_dir, clean)?;
    if let Some(output) = export {
        let package = export_package(cache_dir.clone())?;
        write_export_package(&package, &output, json_lines)?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_extraction_report(&report);
    }

    Ok(())
}

fn find(
    archive: PathBuf,
    query: String,
    json: bool,
    json_lines: bool,
    limit: Option<usize>,
) -> Result<()> {
    let modules = find_module_summaries(archive, &query, limit)?;
    print_module_summaries(&modules, json, json_lines)
}

fn relations(
    archive: PathBuf,
    target: String,
    json: bool,
    json_lines: bool,
    limit: Option<usize>,
) -> Result<()> {
    let matches = relation_matches(archive, &target, limit)?;
    print_relation_matches(&matches, json, json_lines)
}

fn relation_target_stats(archive: PathBuf, limit: usize, json: bool) -> Result<()> {
    let report = relation_stats(archive, limit)?;
    print_relation_stats(&report, json)
}

fn unresolved(archive: PathBuf, relationship: String, limit: usize, json: bool) -> Result<()> {
    let report = unresolved_relations(archive, &relationship, limit)?;
    print_unresolved_relations(&report, json)
}

fn inspect(
    archive: PathBuf,
    identifier: String,
    version: Option<String>,
    latest: bool,
    json: bool,
    limit: Option<usize>,
    reverse_limit: Option<usize>,
) -> Result<()> {
    let report = inspect_module(
        archive,
        &identifier,
        version.as_deref(),
        latest,
        limit,
        reverse_limit,
    )?;
    print_module_inspection(&report, json)
}

fn compare(left: PathBuf, right: PathBuf, json: bool) -> Result<()> {
    let report = compare_archives(left, right)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_compare_report(&report);
    }

    Ok(())
}

fn completions(shell: Shell) -> Result<()> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    clap_complete::generate(shell, &mut command, name, &mut std::io::stdout());
    Ok(())
}
