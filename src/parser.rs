use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::archive::{archive_kind, load_archive, TextEntry};
use crate::model::{
    clean_string, collection_len, has_text, has_value, value_to_text, BenchReport,
    CompareDifference, CompareReport, ExportPackage, IdentifierCount, MinimalModule,
    ModuleInspection, ModuleSummary, ParseError, ParseReport, ParsedModule, RelationMatch,
    RelationStatsReport, RelationTargetCount, TimingStats, UnresolvedRelationReport,
    UnresolvedRelationTarget,
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
    if !archive.exists() {
        bail!("archive path does not exist: {}", archive.display());
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

    let parsed: Vec<Result<ParsedModule, ParseError>> =
        ckan_entries.par_iter().map(parse_module_entry).collect();

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

pub fn latest_module_summaries(
    archive: PathBuf,
    limit: Option<usize>,
) -> Result<Vec<ModuleSummary>> {
    let parsed = parse_archive_details(archive)?;
    let mut latest_by_identifier = BTreeMap::<String, &ParsedModule>::new();

    for module in &parsed.modules {
        let Some(identifier) = module.identifier.as_ref() else {
            continue;
        };

        latest_by_identifier
            .entry(identifier.clone())
            .and_modify(|current| {
                if compare_version_text(&module.version, &current.version).is_gt() {
                    *current = module;
                }
            })
            .or_insert(module);
    }

    let mut modules = latest_by_identifier
        .into_values()
        .map(ModuleSummary::from)
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.identifier.cmp(&right.identifier));

    if let Some(limit) = limit {
        modules.truncate(limit);
    }

    Ok(modules)
}

pub fn export_package(archive: PathBuf) -> Result<ExportPackage> {
    let parsed = parse_archive_details(archive)?;
    let modules = parsed
        .modules
        .iter()
        .map(ModuleSummary::from)
        .collect::<Vec<_>>();

    Ok(ExportPackage {
        schema_version: 1,
        source: parsed.report.archive.clone(),
        report: parsed.report,
        modules,
    })
}

pub fn find_module_summaries(
    archive: PathBuf,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<ModuleSummary>> {
    let query = query.to_lowercase();
    let parsed = parse_archive_details(archive)?;
    let mut matches = parsed
        .modules
        .iter()
        .filter(|module| module_matches(module, &query))
        .map(ModuleSummary::from)
        .collect::<Vec<_>>();

    if let Some(limit) = limit {
        matches.truncate(limit);
    }

    Ok(matches)
}

pub fn relation_matches(
    archive: PathBuf,
    target: &str,
    limit: Option<usize>,
) -> Result<Vec<RelationMatch>> {
    let target_lower = target.to_lowercase();
    let parsed = parse_archive_details(archive)?;
    let mut matches = Vec::new();

    for module in &parsed.modules {
        collect_module_relation_matches(&mut matches, module, &[target_lower.as_str()]);
    }

    if let Some(limit) = limit {
        matches.truncate(limit);
    }

    Ok(matches)
}

pub fn relation_stats(archive: PathBuf, limit: usize) -> Result<RelationStatsReport> {
    let parsed = parse_archive_details(archive)?;
    let mut counts = BTreeMap::new();

    for module in &parsed.modules {
        count_relation_targets(&mut counts, "depends", &module.dependency_names);
        count_relation_targets(&mut counts, "recommends", &module.recommendation_names);
        count_relation_targets(&mut counts, "suggests", &module.suggestion_names);
        count_relation_targets(&mut counts, "conflicts", &module.conflict_names);
        count_relation_targets(&mut counts, "provides", &module.provided_names);
    }

    let mut targets = counts
        .into_iter()
        .map(|((relationship, target), count)| RelationTargetCount {
            relationship,
            target,
            count,
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.relationship.cmp(&right.relationship))
            .then_with(|| left.target.cmp(&right.target))
    });
    targets.truncate(limit);

    Ok(RelationStatsReport {
        archive: parsed.report.archive,
        limit,
        targets,
    })
}

pub fn unresolved_relations(
    archive: PathBuf,
    relationship: &str,
    limit: usize,
) -> Result<UnresolvedRelationReport> {
    let parsed = parse_archive_details(archive)?;
    let relationship = relationship.to_lowercase();
    let mut provided = BTreeSet::new();

    for module in &parsed.modules {
        if let Some(identifier) = module.identifier.as_ref() {
            provided.insert(identifier.to_lowercase());
        }
        for provided_name in &module.provided_names {
            provided.insert(provided_name.to_lowercase());
        }
    }

    let mut counts = BTreeMap::<String, usize>::new();
    for module in &parsed.modules {
        let targets = relationship_targets(module, &relationship)?;
        for target in targets {
            if relation_target_is_resolved(target, &provided) {
                continue;
            }
            *counts.entry(target.clone()).or_default() += 1;
        }
    }

    let mut targets = counts
        .into_iter()
        .map(|(target, count)| UnresolvedRelationTarget { target, count })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.target.cmp(&right.target))
    });
    targets.truncate(limit);

    Ok(UnresolvedRelationReport {
        archive: parsed.report.archive,
        relationship,
        limit,
        targets,
    })
}

pub fn inspect_module(
    archive: PathBuf,
    identifier: &str,
    version: Option<&str>,
    latest: bool,
    limit: Option<usize>,
    reverse_limit: Option<usize>,
) -> Result<ModuleInspection> {
    let parsed = parse_archive_details(archive)?;
    let mut modules = parsed
        .modules
        .iter()
        .filter(|module| identifier_matches(module, identifier))
        .filter(|module| version.is_none_or(|version| module.version.as_deref() == Some(version)))
        .collect::<Vec<_>>();

    if latest {
        modules.sort_by(|left, right| compare_version_text(&right.version, &left.version));
        modules.truncate(1);
    } else if let Some(limit) = limit {
        modules.truncate(limit);
    }

    let mut relationship_targets = BTreeSet::from([identifier.to_string()]);
    for module in &modules {
        relationship_targets.extend(module.provided_names.iter().cloned());
    }

    let target_lowers = relationship_targets
        .iter()
        .map(|target| target.to_lowercase())
        .collect::<Vec<_>>();
    let target_refs = target_lowers.iter().map(String::as_str).collect::<Vec<_>>();

    let mut reverse_relationships = Vec::new();
    for module in &parsed.modules {
        collect_module_relation_matches(&mut reverse_relationships, module, &target_refs);
    }

    if let Some(limit) = reverse_limit {
        reverse_relationships.truncate(limit);
    }

    Ok(ModuleInspection {
        query: identifier.to_string(),
        version: version.map(str::to_string),
        relationship_targets: relationship_targets.into_iter().collect(),
        modules: modules
            .into_iter()
            .map(ModuleSummary::from)
            .collect::<Vec<_>>(),
        reverse_relationships,
    })
}

pub fn compare_archives(left: PathBuf, right: PathBuf) -> Result<CompareReport> {
    let left_parse = parse_archive_details(left)?;
    let right_parse = parse_archive_details(right)?;
    let left_report = &left_parse.report;
    let right_report = &right_parse.report;
    let mut differences = Vec::new();

    compare_value(
        &mut differences,
        "relevant_entries",
        left_report.relevant_entries,
        right_report.relevant_entries,
    );
    compare_value(
        &mut differences,
        "ckan_entries",
        left_report.ckan_entries,
        right_report.ckan_entries,
    );
    compare_value(
        &mut differences,
        "parsed_modules",
        left_report.parsed_modules,
        right_report.parsed_modules,
    );
    compare_value(
        &mut differences,
        "unique_identifiers",
        left_report.unique_identifiers,
        right_report.unique_identifiers,
    );
    compare_value(
        &mut differences,
        "duplicate_identifiers",
        left_report.duplicate_identifiers,
        right_report.duplicate_identifiers,
    );
    compare_value(
        &mut differences,
        "missing_identifier",
        left_report.missing_identifier,
        right_report.missing_identifier,
    );
    compare_value(
        &mut differences,
        "dependency_edges",
        left_report.dependency_edges,
        right_report.dependency_edges,
    );
    compare_value(
        &mut differences,
        "recommendation_edges",
        left_report.recommendation_edges,
        right_report.recommendation_edges,
    );
    compare_value(
        &mut differences,
        "suggestion_edges",
        left_report.suggestion_edges,
        right_report.suggestion_edges,
    );
    compare_value(
        &mut differences,
        "conflict_edges",
        left_report.conflict_edges,
        right_report.conflict_edges,
    );
    compare_value(
        &mut differences,
        "provided_identifiers",
        left_report.provided_identifiers,
        right_report.provided_identifiers,
    );
    compare_value(
        &mut differences,
        "modules_with_install",
        left_report.modules_with_install,
        right_report.modules_with_install,
    );
    compare_value(
        &mut differences,
        "modules_with_resources",
        left_report.modules_with_resources,
        right_report.modules_with_resources,
    );
    compare_value(
        &mut differences,
        "modules_with_download",
        left_report.modules_with_download,
        right_report.modules_with_download,
    );
    compare_value(
        &mut differences,
        "parse_errors",
        left_report.parse_errors,
        right_report.parse_errors,
    );
    compare_option(
        &mut differences,
        "download_counts",
        left_report.download_counts,
        right_report.download_counts,
    );
    compare_option(
        &mut differences,
        "builds",
        left_report.builds,
        right_report.builds,
    );
    compare_option(
        &mut differences,
        "repositories",
        left_report.repositories,
        right_report.repositories,
    );
    compare_value(
        &mut differences,
        "bytes_read",
        left_report.bytes_read,
        right_report.bytes_read,
    );

    let left_modules = module_fingerprints(&left_parse.modules);
    let right_modules = module_fingerprints(&right_parse.modules);
    let left_only_modules = left_modules
        .difference(&right_modules)
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    let right_only_modules = right_modules
        .difference(&left_modules)
        .take(20)
        .cloned()
        .collect::<Vec<_>>();

    if left_modules != right_modules {
        differences.push(CompareDifference {
            field: "module_fingerprints".to_string(),
            left: format!(
                "{} unique, {} sample-only",
                left_modules.len(),
                left_only_modules.len()
            ),
            right: format!(
                "{} unique, {} sample-only",
                right_modules.len(),
                right_only_modules.len()
            ),
        });
    }

    Ok(CompareReport {
        left: left_report.archive.clone(),
        right: right_report.archive.clone(),
        matching: differences.is_empty(),
        differences,
        left_only_modules,
        right_only_modules,
    })
}

fn relationship_targets<'a>(module: &'a ParsedModule, relationship: &str) -> Result<&'a [String]> {
    match relationship {
        "depends" => Ok(&module.dependency_names),
        "recommends" => Ok(&module.recommendation_names),
        "suggests" => Ok(&module.suggestion_names),
        "conflicts" => Ok(&module.conflict_names),
        "provides" => Ok(&module.provided_names),
        _ => bail!("unsupported relationship: {relationship}"),
    }
}

fn relation_target_is_resolved(target: &str, provided: &BTreeSet<String>) -> bool {
    target
        .split('|')
        .any(|option| provided.contains(&option.to_lowercase()))
}

fn count_relation_targets(
    counts: &mut BTreeMap<(String, String), usize>,
    relationship: &str,
    targets: &[String],
) {
    for target in targets {
        *counts
            .entry((relationship.to_string(), target.clone()))
            .or_default() += 1;
    }
}

fn collect_module_relation_matches(
    matches: &mut Vec<RelationMatch>,
    module: &ParsedModule,
    target_lowers: &[&str],
) {
    collect_relation_matches(
        matches,
        "depends",
        target_lowers,
        &module.dependency_names,
        module,
    );
    collect_relation_matches(
        matches,
        "recommends",
        target_lowers,
        &module.recommendation_names,
        module,
    );
    collect_relation_matches(
        matches,
        "suggests",
        target_lowers,
        &module.suggestion_names,
        module,
    );
    collect_relation_matches(
        matches,
        "conflicts",
        target_lowers,
        &module.conflict_names,
        module,
    );
    collect_relation_matches(
        matches,
        "provides",
        target_lowers,
        &module.provided_names,
        module,
    );
}

fn collect_relation_matches(
    matches: &mut Vec<RelationMatch>,
    relationship: &str,
    target_lowers: &[&str],
    relation_names: &[String],
    module: &ParsedModule,
) {
    for relation_name in relation_names {
        if target_lowers.iter().any(|target_lower| {
            relation_name
                .split('|')
                .any(|name| name.eq_ignore_ascii_case(target_lower))
        }) {
            if matches.iter().any(|existing| {
                existing.relationship == relationship
                    && existing.target == *relation_name
                    && existing.module.path == module.path
            }) {
                continue;
            }

            matches.push(RelationMatch {
                relationship: relationship.to_string(),
                target: relation_name.clone(),
                module: ModuleSummary::from(module),
            });
        }
    }
}

fn identifier_matches(module: &ParsedModule, identifier: &str) -> bool {
    module
        .identifier
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case(identifier))
}

fn module_matches(module: &ParsedModule, query: &str) -> bool {
    text_matches(&module.identifier, query)
        || text_matches(&module.name, query)
        || text_matches(&module.version, query)
}

fn text_matches(value: &Option<String>, query: &str) -> bool {
    value
        .as_deref()
        .is_some_and(|text| text.to_lowercase().contains(query))
}

fn parse_module_entry(entry: &&TextEntry) -> Result<ParsedModule, ParseError> {
    let raw =
        serde_json::from_str::<MinimalModule>(&entry.contents).map_err(|error| ParseError {
            path: entry.path.clone(),
            error: error.to_string(),
        })?;
    let dependency_names = relationship_names(raw.depends.as_ref());
    let recommendation_names = relationship_names(raw.recommends.as_ref());
    let suggestion_names = relationship_names(raw.suggests.as_ref());
    let conflict_names = relationship_names(raw.conflicts.as_ref());
    let provided_names = relationship_names(raw.provides.as_ref());

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
        dependency_edges: dependency_names.len(),
        recommendation_edges: recommendation_names.len(),
        suggestion_edges: suggestion_names.len(),
        conflict_edges: conflict_names.len(),
        provided_identifiers: provided_names.len(),
        dependency_names,
        recommendation_names,
        suggestion_names,
        conflict_names,
        provided_names,
    })
}

fn relationship_names(value: Option<&Value>) -> Vec<String> {
    let mut names = match value {
        Some(Value::Array(items)) => items.iter().filter_map(relationship_name).collect(),
        Some(item) => relationship_name(item).into_iter().collect(),
        None => Vec::new(),
    };
    names.sort();
    names
}

fn relationship_name(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        Value::Object(obj) => {
            if let Some(name) = obj
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(str::to_string)
                .filter(|text| !text.is_empty())
            {
                return Some(name);
            }

            let mut any_of = obj
                .get("any_of")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(relationship_name)
                .collect::<Vec<_>>();
            any_of.sort();

            (!any_of.is_empty()).then(|| any_of.join("|"))
        }
        _ => None,
    }
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

fn module_fingerprints(modules: &[ParsedModule]) -> BTreeSet<String> {
    modules.iter().map(module_fingerprint).collect()
}

fn module_fingerprint(module: &ParsedModule) -> String {
    format!(
        "id={}|name={}|version={}|spec={}|abstract={}|author={}|license={}|resources={}|install={}|download={}|ksp={}|ksp_min={}|ksp_max={}|depends={}:{}|recommends={}:{}|suggests={}:{}|conflicts={}:{}|provides={}:{}",
        optional_text(&module.identifier),
        optional_text(&module.name),
        optional_text(&module.version),
        optional_text(&module.spec_version),
        has_text(&module.abstract_text),
        module.author_count,
        module.license_count,
        module.resource_count,
        module.install_steps,
        module.has_download,
        optional_text(&module.ksp_version),
        optional_text(&module.ksp_version_min),
        optional_text(&module.ksp_version_max),
        module.dependency_edges,
        module.dependency_names.join(","),
        module.recommendation_edges,
        module.recommendation_names.join(","),
        module.suggestion_edges,
        module.suggestion_names.join(","),
        module.conflict_edges,
        module.conflict_names.join(","),
        module.provided_identifiers,
        module.provided_names.join(",")
    )
}

fn optional_text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct VersionKey {
    epoch: u64,
    parts: Vec<VersionPart>,
}

impl Ord for VersionKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.epoch
            .cmp(&other.epoch)
            .then_with(|| compare_version_parts(&self.parts, &other.parts))
    }
}

impl PartialOrd for VersionKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum VersionPart {
    Number(u64),
    Text(String),
}

fn compare_version_text(left: &Option<String>, right: &Option<String>) -> Ordering {
    version_key(left.as_deref()).cmp(&version_key(right.as_deref()))
}

fn version_key(version: Option<&str>) -> VersionKey {
    let version = version.unwrap_or_default();
    let (epoch, raw_version) = version
        .split_once(':')
        .and_then(|(epoch, rest)| epoch.parse::<u64>().ok().map(|epoch| (epoch, rest)))
        .unwrap_or((0, version));
    let raw_version = raw_version
        .strip_prefix('v')
        .or_else(|| raw_version.strip_prefix('V'))
        .unwrap_or(raw_version);

    VersionKey {
        epoch,
        parts: version_parts(raw_version),
    }
}

fn version_parts(version: &str) -> Vec<VersionPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_is_digit = None;

    for ch in version.chars() {
        if !ch.is_ascii_alphanumeric() {
            push_version_part(&mut parts, &mut current, current_is_digit);
            current_is_digit = None;
            continue;
        }

        let is_digit = ch.is_ascii_digit();
        if current_is_digit.is_some_and(|digit| digit != is_digit) {
            push_version_part(&mut parts, &mut current, current_is_digit);
        }

        current_is_digit = Some(is_digit);
        current.push(ch);
    }

    push_version_part(&mut parts, &mut current, current_is_digit);
    parts
}

fn push_version_part(
    parts: &mut Vec<VersionPart>,
    current: &mut String,
    current_is_digit: Option<bool>,
) {
    if current.is_empty() {
        return;
    }

    if current_is_digit.unwrap_or(false) {
        parts.push(VersionPart::Number(current.parse().unwrap_or(0)));
    } else {
        parts.push(VersionPart::Text(current.to_lowercase()));
    }

    current.clear();
}

fn compare_version_parts(left: &[VersionPart], right: &[VersionPart]) -> Ordering {
    for index in 0..left.len().max(right.len()) {
        match (left.get(index), right.get(index)) {
            (Some(VersionPart::Number(left)), Some(VersionPart::Number(right))) => {
                match left.cmp(right) {
                    Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
            (Some(VersionPart::Text(left)), Some(VersionPart::Text(right))) => {
                match left.cmp(right) {
                    Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
            (Some(VersionPart::Number(_)), Some(VersionPart::Text(_))) => return Ordering::Greater,
            (Some(VersionPart::Text(_)), Some(VersionPart::Number(_))) => return Ordering::Less,
            (Some(VersionPart::Number(left)), None) if *left == 0 => {}
            (None, Some(VersionPart::Number(right))) if *right == 0 => {}
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }

    Ordering::Equal
}

fn compare_value<T>(differences: &mut Vec<CompareDifference>, field: &str, left: T, right: T)
where
    T: std::fmt::Display + PartialEq,
{
    if left != right {
        differences.push(CompareDifference {
            field: field.to_string(),
            left: left.to_string(),
            right: right.to_string(),
        });
    }
}

fn compare_option<T>(
    differences: &mut Vec<CompareDifference>,
    field: &str,
    left: Option<T>,
    right: Option<T>,
) where
    T: std::fmt::Display + PartialEq,
{
    if left != right {
        differences.push(CompareDifference {
            field: field.to_string(),
            left: format_optional(left),
            right: format_optional(right),
        });
    }
}

fn format_optional<T>(value: Option<T>) -> String
where
    T: std::fmt::Display,
{
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
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
        assert_eq!(
            parsed.dependency_names,
            vec!["Harmony".to_string(), "ModuleManager".to_string()]
        );
        assert_eq!(
            parsed.recommendation_names,
            vec!["ToolbarController".to_string()]
        );
        assert_eq!(parsed.conflict_names, vec!["OldExample".to_string()]);
        assert_eq!(parsed.provided_names, vec!["ExampleVirtual".to_string()]);
    }

    #[test]
    fn identifies_relevant_entries() {
        assert!(is_relevant_entry("ModuleManager.ckan"));
        assert!(is_relevant_entry("CKAN-meta/download_counts.json"));
        assert!(is_relevant_entry("CKAN-meta/builds.json"));
        assert!(is_relevant_entry("CKAN-meta/repositories.json"));
        assert!(!is_relevant_entry("README.md"));
    }

    #[test]
    fn extracts_any_of_relationship_names_as_one_edge() {
        let value = serde_json::json!([
            {
                "any_of": [
                    { "name": "FirstOption" },
                    { "name": "SecondOption" }
                ]
            },
            { "name": "RequiredMod" }
        ]);

        let names = relationship_names(Some(&value));

        assert_eq!(
            names,
            vec![
                "FirstOption|SecondOption".to_string(),
                "RequiredMod".to_string()
            ]
        );
    }

    #[test]
    fn relation_lookup_matches_any_of_members() {
        let module = ParsedModule {
            path: "Example.ckan".to_string(),
            identifier: Some("Example".to_string()),
            name: Some("Example".to_string()),
            version: Some("1.0".to_string()),
            spec_version: Some("v1.0".to_string()),
            abstract_text: None,
            author_count: 0,
            license_count: 0,
            resource_count: 0,
            install_steps: 0,
            has_download: false,
            ksp_version: None,
            ksp_version_min: None,
            ksp_version_max: None,
            dependency_edges: 1,
            recommendation_edges: 0,
            suggestion_edges: 0,
            conflict_edges: 0,
            provided_identifiers: 0,
            dependency_names: vec!["FirstOption|SecondOption".to_string()],
            recommendation_names: Vec::new(),
            suggestion_names: Vec::new(),
            conflict_names: Vec::new(),
            provided_names: Vec::new(),
        };
        let mut matches = Vec::new();

        collect_relation_matches(
            &mut matches,
            "depends",
            &["secondoption"],
            &module.dependency_names,
            &module,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].target, "FirstOption|SecondOption");
    }

    #[test]
    fn compares_numeric_version_chunks() {
        assert_eq!(
            compare_version_text(&Some("v1.13".to_string()), &Some("v1.9".to_string())),
            Ordering::Greater
        );
        assert_eq!(
            compare_version_text(&Some("3:v4.13".to_string()), &Some("2:v999".to_string())),
            Ordering::Greater
        );
        assert_eq!(
            compare_version_text(&Some("1.0.0".to_string()), &Some("1".to_string())),
            Ordering::Equal
        );
    }
}
