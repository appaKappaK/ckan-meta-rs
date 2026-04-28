use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use anyhow::Result;

use crate::model::{
    BenchReport, CompareReport, ExportPackage, ModuleInspection, ModuleSummary, ParseReport,
    RelationMatch, RelationStatsReport, TimingStats,
};

pub fn print_report(report: &ParseReport, max_errors: usize) {
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
    print_relationship_counts(
        report.dependency_edges,
        report.recommendation_edges,
        report.suggestion_edges,
        report.conflict_edges,
        report.provided_identifiers,
    );
    println!(
        "Field coverage: abstract={} author={} license={} install={} resources={} download={}",
        report.modules_with_abstract,
        report.modules_with_author,
        report.modules_with_license,
        report.modules_with_install,
        report.modules_with_resources,
        report.modules_with_download
    );
    println!(
        "KSP compatibility fields: exact={} min={} max={}",
        report.modules_with_ksp_version,
        report.modules_with_ksp_version_min,
        report.modules_with_ksp_version_max
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

pub fn print_bench_report(report: &BenchReport) {
    println!("Archive: {}", report.archive);
    println!("Type: {}", report.archive_kind);
    println!("Warmups: {}", report.warmups);
    println!("Runs: {}", report.runs);
    println!("Parsed modules: {}", report.sample.parsed_modules);
    println!("Unique identifiers: {}", report.sample.unique_identifiers);
    print_relationship_counts(
        report.sample.dependency_edges,
        report.sample.recommendation_edges,
        report.sample.suggestion_edges,
        report.sample.conflict_edges,
        report.sample.provided_identifiers,
    );
    println!("Parse errors: {}", report.sample.parse_errors);
    println!("Bytes read per run: {}", report.sample.bytes_read);
    println!();
    println!("Timing statistics:");
    print_timing_stats("read", &report.read_ms);
    print_timing_stats("parse", &report.parse_ms);
    print_timing_stats("total", &report.elapsed_ms);
}

pub fn print_module_summaries(
    modules: &[ModuleSummary],
    json: bool,
    json_lines: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(modules)?);
    } else if json_lines {
        for module in modules {
            println!("{}", serde_json::to_string(module)?);
        }
    } else {
        print_module_table(modules);
    }

    Ok(())
}

pub fn write_export_package(
    package: &ExportPackage,
    output: &Path,
    json_lines: bool,
) -> Result<()> {
    let writer: Box<dyn Write> = if output == Path::new("-") {
        Box::new(io::stdout().lock())
    } else {
        Box::new(File::create(output)?)
    };
    let mut writer = BufWriter::new(writer);

    if json_lines {
        for module in &package.modules {
            serde_json::to_writer(&mut writer, module)?;
            writeln!(writer)?;
        }
    } else {
        serde_json::to_writer_pretty(&mut writer, package)?;
        writeln!(writer)?;
    }

    writer.flush()?;
    Ok(())
}

pub fn print_compare_report(report: &CompareReport) {
    println!("Left: {}", report.left);
    println!("Right: {}", report.right);
    println!("Matching: {}", report.matching);

    if !report.differences.is_empty() {
        println!();
        println!("{:<28} {:>16} {:>16}", "Field", "Left", "Right");
        for difference in &report.differences {
            println!(
                "{:<28} {:>16} {:>16}",
                difference.field, difference.left, difference.right
            );
        }
    }

    if !report.left_only_modules.is_empty() {
        println!();
        println!("Left-only module fingerprint samples:");
        for module in &report.left_only_modules {
            println!("  {}", module);
        }
    }

    if !report.right_only_modules.is_empty() {
        println!();
        println!("Right-only module fingerprint samples:");
        for module in &report.right_only_modules {
            println!("  {}", module);
        }
    }
}

pub fn print_relation_matches(
    matches: &[RelationMatch],
    json: bool,
    json_lines: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(matches)?);
    } else if json_lines {
        for item in matches {
            println!("{}", serde_json::to_string(item)?);
        }
    } else {
        print_relation_table(matches);
    }

    Ok(())
}

pub fn print_relation_stats(report: &RelationStatsReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Archive: {}", report.archive);
    println!("Limit: {}", report.limit);
    println!();
    println!("{:<10} {:<48} {:>8}", "Relation", "Target", "Count");
    for target in &report.targets {
        println!(
            "{:<10} {:<48} {:>8}",
            target.relationship,
            truncate(&target.target, 48),
            target.count
        );
    }

    Ok(())
}

pub fn print_module_inspection(report: &ModuleInspection, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Query: {}", report.query);
    if let Some(version) = report.version.as_ref() {
        println!("Version: {version}");
    }
    println!(
        "Relationship targets: {}",
        report.relationship_targets.join(", ")
    );

    println!();
    println!("Matched modules:");
    print_module_table(&report.modules);

    if !report.modules.is_empty() {
        println!();
        println!("Direct relationships:");
        for module in &report.modules {
            print_module_relationships(module);
        }
    }

    println!();
    println!("Reverse relationships:");
    print_relation_table(&report.reverse_relationships);

    Ok(())
}

fn print_module_table(modules: &[ModuleSummary]) {
    println!(
        "{:<32} {:<16} {:>3} {:>3} {:>3} {:>3} {:>3} {:<13}",
        "Identifier", "Version", "Dep", "Rec", "Sug", "Con", "Ins", "KSP"
    );
    for module in modules {
        println!(
            "{:<32} {:<16} {:>3} {:>3} {:>3} {:>3} {:>3} {:<13}",
            truncate(module.identifier.as_deref().unwrap_or("-"), 32),
            truncate(module.version.as_deref().unwrap_or("-"), 16),
            module.dependency_edges,
            module.recommendation_edges,
            module.suggestion_edges,
            module.conflict_edges,
            module.install_steps,
            truncate(ksp_compat(module).as_str(), 13)
        );
    }
}

fn print_relation_table(matches: &[RelationMatch]) {
    println!(
        "{:<10} {:<32} {:<32} {:<16} {:<13}",
        "Relation", "Target", "Identifier", "Version", "KSP"
    );
    for item in matches {
        println!(
            "{:<10} {:<32} {:<32} {:<16} {:<13}",
            item.relationship,
            truncate(&item.target, 32),
            truncate(item.module.identifier.as_deref().unwrap_or("-"), 32),
            truncate(item.module.version.as_deref().unwrap_or("-"), 16),
            truncate(ksp_compat(&item.module).as_str(), 13)
        );
    }
}

fn print_module_relationships(module: &ModuleSummary) {
    println!(
        "{} {}",
        module.identifier.as_deref().unwrap_or("-"),
        module.version.as_deref().unwrap_or("-")
    );
    println!("  depends: {}", join_or_dash(&module.dependency_names));
    println!(
        "  recommends: {}",
        join_or_dash(&module.recommendation_names)
    );
    println!("  suggests: {}", join_or_dash(&module.suggestion_names));
    println!("  conflicts: {}", join_or_dash(&module.conflict_names));
    println!("  provides: {}", join_or_dash(&module.provided_names));
}

fn print_relationship_counts(
    dependency_edges: usize,
    recommendation_edges: usize,
    suggestion_edges: usize,
    conflict_edges: usize,
    provided_identifiers: usize,
) {
    println!(
        "Relationship edges: depends={} recommends={} suggests={} conflicts={} provides={}",
        dependency_edges,
        recommendation_edges,
        suggestion_edges,
        conflict_edges,
        provided_identifiers
    );
}

fn join_or_dash(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
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

fn ksp_compat(module: &ModuleSummary) -> String {
    if let Some(version) = module.ksp_version.as_ref() {
        return version.clone();
    }

    match (&module.ksp_version_min, &module.ksp_version_max) {
        (Some(min), Some(max)) => format!("{min}-{max}"),
        (Some(min), None) => format!("{min}+"),
        (None, Some(max)) => format!("<={max}"),
        (None, None) => "-".to_string(),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut value = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    value.push('~');
    value
}
