use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde_json::Value;

use crate::archive::{archive_kind, load_archive, ArchiveLoad, TextEntry};
use crate::model::{
    clean_string, collection_len, has_text, has_value, value_to_text, BenchReport, CatalogIndex,
    CatalogIndexReport, CatalogModule, CatalogProvider, CatalogRelation, CompareDifference,
    CompareReport, ExportPackage, IdentifierCount, MinimalModule, ModuleInspection, ModuleSummary,
    ParseError, ParseReport, ParsedModule, RelationMatch, RelationStatsReport, RelationTargetCount,
    TimingStats, UnresolvedRelationReport, UnresolvedRelationTarget,
};
use crate::repository_cache::load_repository_caches;

#[derive(Debug)]
pub struct ArchiveParse {
    pub report: ParseReport,
    pub modules: Vec<ParsedModule>,
    pub download_counts: BTreeMap<String, u64>,
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
    parse_loaded_details(archive.display().to_string(), archive_kind, loaded, started)
}

fn parse_repository_cache_details(paths: &[PathBuf]) -> Result<ArchiveParse> {
    let started = Instant::now();
    let loaded = load_repository_caches(paths)?;
    let source = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(";");
    parse_loaded_details(source, "repository-cache", loaded, started)
}

fn parse_loaded_details(
    source: String,
    source_kind: &str,
    loaded: ArchiveLoad,
    started: Instant,
) -> Result<ArchiveParse> {
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
    let download_counts = parse_download_counts(&loaded.entries);
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
        archive: source,
        archive_kind: source_kind.to_string(),
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

    Ok(ArchiveParse {
        report,
        modules,
        download_counts,
    })
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

pub fn catalog_index(archive: PathBuf, latest_only: bool) -> Result<CatalogIndex> {
    let parsed = parse_archive_details(archive)?;
    Ok(build_catalog_index(parsed, latest_only, None))
}

pub fn catalog_index_from_repository_caches(
    paths: &[PathBuf],
    latest_only: bool,
    source_fingerprint: Option<String>,
) -> Result<CatalogIndex> {
    let parsed = parse_repository_cache_details(paths)?;
    Ok(build_catalog_index(parsed, latest_only, source_fingerprint))
}

fn build_catalog_index(
    parsed: ArchiveParse,
    latest_only: bool,
    source_fingerprint: Option<String>,
) -> CatalogIndex {
    let version_counts = version_counts_by_identifier(&parsed.modules);
    let latest_paths = latest_paths_by_stability(&parsed.modules);

    let modules = parsed
        .modules
        .iter()
        .filter(|module| !latest_only || module_is_latest_for_any_stability(module, &latest_paths))
        .filter_map(|module| {
            catalog_module(
                module,
                &version_counts,
                &latest_paths,
                &parsed.download_counts,
            )
        })
        .collect::<Vec<_>>();
    let relations = parsed
        .modules
        .iter()
        .filter(|module| !latest_only || module_is_latest_for_any_stability(module, &latest_paths))
        .flat_map(catalog_relations_from_parsed)
        .collect::<Vec<_>>();
    let providers = modules
        .iter()
        .flat_map(catalog_providers)
        .collect::<Vec<_>>();

    CatalogIndex {
        schema_version: 2,
        source: parsed.report.archive.clone(),
        source_fingerprint,
        generated_by: env!("CARGO_PKG_NAME").to_string(),
        report: CatalogIndexReport {
            parsed_modules: parsed.report.parsed_modules,
            unique_identifiers: parsed.report.unique_identifiers,
            latest_modules: latest_paths.len(),
            dependency_edges: parsed.report.dependency_edges,
            recommendation_edges: parsed.report.recommendation_edges,
            suggestion_edges: parsed.report.suggestion_edges,
            conflict_edges: parsed.report.conflict_edges,
            provided_identifiers: parsed.report.provided_identifiers,
            parse_errors: parsed.report.parse_errors,
            read_ms: parsed.report.read_ms,
            parse_ms: parsed.report.parse_ms,
            elapsed_ms: parsed.report.elapsed_ms,
        },
        modules,
        relations,
        providers,
    }
}

fn module_is_latest_for_any_stability(
    module: &ParsedModule,
    latest_paths: &BTreeMap<String, LatestModulePaths>,
) -> bool {
    let Some(identifier) = module
        .identifier
        .as_ref()
        .filter(|identifier| !identifier.is_empty())
    else {
        return false;
    };
    latest_paths
        .get(identifier)
        .is_some_and(|paths| paths.contains(&module.path))
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
        description: raw.description.map(clean_string),
        authors: string_values(raw.author.as_ref()),
        licenses: string_values(raw.license.as_ref()),
        kind: raw.kind.map(clean_string),
        release_status: normalize_release_status(raw.release_status.as_deref()),
        release_date: raw.release_date.map(clean_string),
        download_size: raw.download_size.as_ref().and_then(value_to_u64),
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

fn catalog_module(
    module: &ParsedModule,
    version_counts: &BTreeMap<String, usize>,
    latest_paths: &BTreeMap<String, LatestModulePaths>,
    download_counts: &BTreeMap<String, u64>,
) -> Option<CatalogModule> {
    let identifier = module.identifier.as_ref()?.trim();
    if identifier.is_empty() {
        return None;
    }

    let name = module
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(identifier)
        .to_string();

    Some(CatalogModule {
        path: module.path.clone(),
        identifier: identifier.to_string(),
        name,
        version: module.version.clone(),
        spec_version: module.spec_version.clone(),
        abstract_text: module.abstract_text.clone(),
        description: module.description.clone(),
        authors: module.authors.clone(),
        licenses: module.licenses.clone(),
        kind: module.kind.clone(),
        release_status: module.release_status.clone(),
        release_date: module.release_date.clone(),
        download_size: module.download_size,
        download_count: download_counts.get(identifier).copied(),
        ksp_version: module.ksp_version.clone(),
        ksp_version_min: module.ksp_version_min.clone(),
        ksp_version_max: module.ksp_version_max.clone(),
        dependency_names: split_relationship_options(&module.dependency_names),
        recommendation_names: split_relationship_options(&module.recommendation_names),
        suggestion_names: split_relationship_options(&module.suggestion_names),
        conflict_names: split_relationship_options(&module.conflict_names),
        provided_names: split_relationship_options(&module.provided_names),
        version_count: version_counts.get(identifier).copied().unwrap_or(1),
        is_latest: latest_paths
            .get(identifier)
            .and_then(|paths| paths.development.as_ref())
            .is_some_and(|path| path == &module.path),
        is_latest_stable: latest_paths
            .get(identifier)
            .and_then(|paths| paths.stable.as_ref())
            .is_some_and(|path| path == &module.path),
        is_latest_testing: latest_paths
            .get(identifier)
            .and_then(|paths| paths.testing.as_ref())
            .is_some_and(|path| path == &module.path),
        is_latest_development: latest_paths
            .get(identifier)
            .and_then(|paths| paths.development.as_ref())
            .is_some_and(|path| path == &module.path),
    })
}

fn catalog_relations_from_parsed(module: &ParsedModule) -> Vec<CatalogRelation> {
    let Some(identifier) = module
        .identifier
        .as_deref()
        .map(str::trim)
        .filter(|identifier| !identifier.is_empty())
    else {
        return Vec::new();
    };
    let source_name = module
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(identifier)
        .to_string();

    let mut relations = Vec::new();
    append_catalog_relations(
        &mut relations,
        identifier,
        &source_name,
        &module.version,
        "depends",
        &module.dependency_names,
    );
    append_catalog_relations(
        &mut relations,
        identifier,
        &source_name,
        &module.version,
        "recommends",
        &module.recommendation_names,
    );
    append_catalog_relations(
        &mut relations,
        identifier,
        &source_name,
        &module.version,
        "suggests",
        &module.suggestion_names,
    );
    append_catalog_relations(
        &mut relations,
        identifier,
        &source_name,
        &module.version,
        "conflicts",
        &module.conflict_names,
    );
    relations
}

fn append_catalog_relations(
    relations: &mut Vec<CatalogRelation>,
    source_identifier: &str,
    source_name: &str,
    source_version: &Option<String>,
    relationship: &str,
    targets: &[String],
) {
    for raw_target in targets {
        for target in raw_target
            .split('|')
            .map(str::trim)
            .filter(|target| !target.is_empty())
        {
            relations.push(CatalogRelation {
                relationship: relationship.to_string(),
                source_identifier: source_identifier.to_string(),
                source_name: source_name.to_string(),
                source_version: source_version.clone(),
                target: target.to_string(),
                raw_target: raw_target.clone(),
            });
        }
    }
}

fn catalog_providers(module: &CatalogModule) -> Vec<CatalogProvider> {
    module
        .provided_names
        .iter()
        .map(|provided| CatalogProvider {
            provided: provided.clone(),
            identifier: module.identifier.clone(),
            name: module.name.clone(),
            version: module.version.clone(),
        })
        .collect()
}

fn version_counts_by_identifier(modules: &[ParsedModule]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for module in modules {
        if let Some(identifier) = module
            .identifier
            .as_ref()
            .filter(|identifier| !identifier.is_empty())
        {
            *counts.entry(identifier.clone()).or_default() += 1;
        }
    }
    counts
}

#[derive(Debug, Default)]
struct LatestModulePaths {
    stable: Option<String>,
    testing: Option<String>,
    development: Option<String>,
}

impl LatestModulePaths {
    fn contains(&self, path: &str) -> bool {
        self.stable.as_deref() == Some(path)
            || self.testing.as_deref() == Some(path)
            || self.development.as_deref() == Some(path)
    }
}

fn latest_paths_by_stability(modules: &[ParsedModule]) -> BTreeMap<String, LatestModulePaths> {
    let mut latest_by_identifier = BTreeMap::<String, [Option<&ParsedModule>; 3]>::new();
    for module in modules {
        let Some(identifier) = module
            .identifier
            .as_ref()
            .filter(|identifier| !identifier.is_empty())
        else {
            continue;
        };
        let release_rank = release_status_rank(&module.release_status);
        let latest = latest_by_identifier
            .entry(identifier.clone())
            .or_insert([None, None, None]);
        for candidate in latest.iter_mut().skip(release_rank) {
            if candidate.is_none_or(|current| {
                compare_version_text(&module.version, &current.version).is_gt()
            }) {
                *candidate = Some(module);
            }
        }
    }

    latest_by_identifier
        .into_iter()
        .map(|(identifier, modules)| {
            (
                identifier,
                LatestModulePaths {
                    stable: modules[0].map(|module| module.path.clone()),
                    testing: modules[1].map(|module| module.path.clone()),
                    development: modules[2].map(|module| module.path.clone()),
                },
            )
        })
        .collect()
}

fn normalize_release_status(status: Option<&str>) -> String {
    match status
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("development" | "alpha") => "development".to_string(),
        Some("testing" | "beta") => "testing".to_string(),
        _ => "stable".to_string(),
    }
}

fn release_status_rank(status: &str) -> usize {
    match status {
        "development" => 2,
        "testing" => 1,
        _ => 0,
    }
}

fn split_relationship_options(names: &[String]) -> Vec<String> {
    let mut values = names
        .iter()
        .flat_map(|name| name.split('|'))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
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

fn string_values(value: Option<&Value>) -> Vec<String> {
    let mut values = match value {
        Some(Value::Array(items)) => items.iter().filter_map(string_value).collect(),
        Some(item) => string_value(item).into_iter().collect(),
        None => Vec::new(),
    };
    values.sort();
    values
}

fn string_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        other if !other.is_null() => {
            Some(value_to_text(other).trim().to_string()).filter(|text| !text.is_empty())
        }
        _ => None,
    }
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
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

fn parse_download_counts(entries: &[TextEntry]) -> BTreeMap<String, u64> {
    entries
        .iter()
        .find(|entry| entry.path.ends_with("download_counts.json"))
        .and_then(|entry| serde_json::from_str::<Value>(&entry.contents).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .map(|map| {
            map.into_iter()
                .filter_map(|(identifier, count)| {
                    value_to_u64(&count).map(|count| (identifier, count))
                })
                .collect()
        })
        .unwrap_or_default()
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
        "id={}|name={}|version={}|spec={}|abstract={}|description={}|authors={}|licenses={}|kind={}|release_date={}|download_size={}|author={}|license={}|resources={}|install={}|download={}|ksp={}|ksp_min={}|ksp_max={}|depends={}:{}|recommends={}:{}|suggests={}:{}|conflicts={}:{}|provides={}:{}",
        optional_text(&module.identifier),
        optional_text(&module.name),
        optional_text(&module.version),
        optional_text(&module.spec_version),
        has_text(&module.abstract_text),
        has_text(&module.description),
        module.authors.join(","),
        module.licenses.join(","),
        optional_text(&module.kind),
        optional_text(&module.release_date),
        module.download_size.unwrap_or(0),
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
                "description": "Long description",
                "author": ["First", "Second"],
                "license": "MIT",
                "kind": "package",
                "release_status": "beta",
                "version": "1.2.3",
                "release_date": "2026-04-28T00:00:00Z",
                "download_size": 12345,
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
        assert_eq!(parsed.description.as_deref(), Some("Long description"));
        assert_eq!(
            parsed.authors,
            vec!["First".to_string(), "Second".to_string()]
        );
        assert_eq!(parsed.licenses, vec!["MIT".to_string()]);
        assert_eq!(parsed.kind.as_deref(), Some("package"));
        assert_eq!(parsed.release_status, "testing");
        assert_eq!(parsed.release_date.as_deref(), Some("2026-04-28T00:00:00Z"));
        assert_eq!(parsed.download_size, Some(12345));
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
        let mut module = test_module("Example", "1.0", "Example.ckan");
        module.dependency_edges = 1;
        module.dependency_names = vec!["FirstOption|SecondOption".to_string()];
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
    fn catalog_index_modules_mark_latest_and_split_relationship_targets() {
        let mut old_module = test_module("Example", "1.0", "Example-1.0.ckan");
        old_module.dependency_names = vec!["FirstOption|SecondOption".to_string()];
        let mut new_module = test_module("Example", "2.0", "Example-2.0.ckan");
        new_module.provided_names = vec!["ExampleVirtual".to_string()];
        let modules = vec![old_module, new_module];
        let version_counts = version_counts_by_identifier(&modules);
        let latest_paths = latest_paths_by_stability(&modules);
        let download_counts = BTreeMap::from([("Example".to_string(), 42)]);

        let catalog_modules = modules
            .iter()
            .filter_map(|module| {
                catalog_module(module, &version_counts, &latest_paths, &download_counts)
            })
            .collect::<Vec<_>>();
        let relations = modules
            .iter()
            .flat_map(catalog_relations_from_parsed)
            .collect::<Vec<_>>();
        let providers = catalog_modules
            .iter()
            .flat_map(catalog_providers)
            .collect::<Vec<_>>();

        assert_eq!(catalog_modules.len(), 2);
        assert_eq!(catalog_modules[0].download_count, Some(42));
        assert_eq!(catalog_modules[0].version_count, 2);
        assert!(!catalog_modules[0].is_latest);
        assert!(catalog_modules[1].is_latest);
        assert!(catalog_modules[1].is_latest_stable);
        assert!(catalog_modules[1].is_latest_testing);
        assert!(catalog_modules[1].is_latest_development);
        assert_eq!(
            catalog_modules[0].dependency_names,
            vec!["FirstOption".to_string(), "SecondOption".to_string()]
        );
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].raw_target, "FirstOption|SecondOption");
        assert_eq!(relations[1].target, "SecondOption");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provided, "ExampleVirtual");
    }

    #[test]
    fn latest_paths_track_each_release_stability_tolerance() {
        let stable = test_module("Example", "1.0", "Example-1.0.ckan");
        let mut testing = test_module("Example", "2.0-beta", "Example-2.0-beta.ckan");
        testing.release_status = "testing".to_string();
        let mut development = test_module("Example", "3.0-alpha", "Example-3.0-alpha.ckan");
        development.release_status = "development".to_string();

        let latest = latest_paths_by_stability(&[stable, testing, development]);
        let paths = &latest["Example"];

        assert_eq!(paths.stable.as_deref(), Some("Example-1.0.ckan"));
        assert_eq!(paths.testing.as_deref(), Some("Example-2.0-beta.ckan"));
        assert_eq!(paths.development.as_deref(), Some("Example-3.0-alpha.ckan"));
    }

    #[test]
    fn release_status_aliases_match_ckan_values() {
        assert_eq!(normalize_release_status(None), "stable");
        assert_eq!(normalize_release_status(Some("beta")), "testing");
        assert_eq!(normalize_release_status(Some("alpha")), "development");
    }

    fn test_module(identifier: &str, version: &str, path: &str) -> ParsedModule {
        ParsedModule {
            path: path.to_string(),
            identifier: Some(identifier.to_string()),
            name: Some(identifier.to_string()),
            version: Some(version.to_string()),
            spec_version: Some("v1.0".to_string()),
            abstract_text: None,
            description: None,
            authors: Vec::new(),
            licenses: Vec::new(),
            kind: None,
            release_status: "stable".to_string(),
            release_date: None,
            download_size: None,
            author_count: 0,
            license_count: 0,
            resource_count: 0,
            install_steps: 0,
            has_download: false,
            ksp_version: None,
            ksp_version_min: None,
            ksp_version_max: None,
            dependency_edges: 0,
            recommendation_edges: 0,
            suggestion_edges: 0,
            conflict_edges: 0,
            provided_identifiers: 0,
            dependency_names: Vec::new(),
            recommendation_names: Vec::new(),
            suggestion_names: Vec::new(),
            conflict_names: Vec::new(),
            provided_names: Vec::new(),
        }
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
