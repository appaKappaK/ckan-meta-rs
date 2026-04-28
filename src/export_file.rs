use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;

use crate::model::{ExportPackage, ExportValidationReport, ModuleSummary};

pub fn validate_export_file(path: &Path, json_lines: bool) -> Result<ExportValidationReport> {
    if json_lines {
        validate_json_lines(path)
    } else {
        validate_package(path)
    }
}

fn validate_package(path: &Path) -> Result<ExportValidationReport> {
    let file = File::open(path)?;
    let package = serde_json::from_reader::<_, ExportPackage>(file)?;
    Ok(validation_report(
        path,
        "json",
        Some(package.schema_version),
        &package.modules,
    ))
}

fn validate_json_lines(path: &Path) -> Result<ExportValidationReport> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut modules = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        modules.push(serde_json::from_str::<ModuleSummary>(&line)?);
    }

    Ok(validation_report(path, "json-lines", None, &modules))
}

fn validation_report(
    path: &Path,
    format: &str,
    schema_version: Option<u32>,
    modules: &[ModuleSummary],
) -> ExportValidationReport {
    let unique_identifiers = modules
        .iter()
        .filter_map(|module| module.identifier.as_ref())
        .collect::<BTreeSet<_>>()
        .len();

    ExportValidationReport {
        input: path.display().to_string(),
        format: format.to_string(),
        schema_version,
        modules: modules.len(),
        unique_identifiers,
        missing_identifier: modules
            .iter()
            .filter(|module| module.identifier.as_deref().unwrap_or("").is_empty())
            .count(),
        dependency_edges: modules.iter().map(|module| module.dependency_edges).sum(),
        recommendation_edges: modules
            .iter()
            .map(|module| module.recommendation_edges)
            .sum(),
        suggestion_edges: modules.iter().map(|module| module.suggestion_edges).sum(),
        conflict_edges: modules.iter().map(|module| module.conflict_edges).sum(),
        provided_identifiers: modules
            .iter()
            .map(|module| module.provided_identifiers)
            .sum(),
    }
}
