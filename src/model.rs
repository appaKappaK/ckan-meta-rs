use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ParseReport {
    pub archive: String,
    pub archive_kind: String,
    pub archive_entries: usize,
    pub relevant_entries: usize,
    pub ckan_entries: usize,
    pub parsed_modules: usize,
    pub named_modules: usize,
    pub versioned_modules: usize,
    pub spec_versioned_modules: usize,
    pub unique_identifiers: usize,
    pub duplicate_identifiers: usize,
    pub missing_identifier: usize,
    pub dependency_edges: usize,
    pub recommendation_edges: usize,
    pub suggestion_edges: usize,
    pub conflict_edges: usize,
    pub provided_identifiers: usize,
    pub modules_with_abstract: usize,
    pub modules_with_author: usize,
    pub modules_with_license: usize,
    pub modules_with_install: usize,
    pub modules_with_resources: usize,
    pub modules_with_download: usize,
    pub modules_with_ksp_version: usize,
    pub modules_with_ksp_version_min: usize,
    pub modules_with_ksp_version_max: usize,
    pub parse_errors: usize,
    pub download_counts: Option<usize>,
    pub builds: Option<usize>,
    pub repositories: Option<usize>,
    pub bytes_read: u64,
    pub read_ms: u128,
    pub parse_ms: u128,
    pub elapsed_ms: u128,
    pub top_identifiers: Vec<IdentifierCount>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub archive: String,
    pub archive_kind: String,
    pub warmups: usize,
    pub runs: usize,
    pub sample: ParseReport,
    pub read_ms: TimingStats,
    pub parse_ms: TimingStats,
    pub elapsed_ms: TimingStats,
}

#[derive(Debug, Serialize)]
pub struct TimingStats {
    pub min: u128,
    pub max: u128,
    pub avg: f64,
    pub total: u128,
    pub values: Vec<u128>,
}

impl TimingStats {
    pub fn from_values(values: impl Iterator<Item = u128>) -> Self {
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
pub struct IdentifierCount {
    pub identifier: String,
    pub versions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseError {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Deserialize)]
pub struct MinimalModule {
    pub spec_version: Option<Value>,
    pub identifier: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub author: Option<Value>,
    pub license: Option<Value>,
    pub version: Option<Value>,
    pub ksp_version: Option<Value>,
    pub ksp_version_min: Option<Value>,
    pub ksp_version_max: Option<Value>,
    pub depends: Option<Value>,
    pub recommends: Option<Value>,
    pub suggests: Option<Value>,
    pub conflicts: Option<Value>,
    pub provides: Option<Value>,
    pub resources: Option<Value>,
    pub install: Option<Value>,
    pub download: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedModule {
    pub path: String,
    pub identifier: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub spec_version: Option<String>,
    pub abstract_text: Option<String>,
    pub author_count: usize,
    pub license_count: usize,
    pub resource_count: usize,
    pub install_steps: usize,
    pub has_download: bool,
    pub ksp_version: Option<String>,
    pub ksp_version_min: Option<String>,
    pub ksp_version_max: Option<String>,
    pub dependency_edges: usize,
    pub recommendation_edges: usize,
    pub suggestion_edges: usize,
    pub conflict_edges: usize,
    pub provided_identifiers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleSummary {
    pub path: String,
    pub identifier: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub spec_version: Option<String>,
    pub has_abstract: bool,
    pub author_count: usize,
    pub license_count: usize,
    pub resource_count: usize,
    pub install_steps: usize,
    pub has_download: bool,
    pub ksp_version: Option<String>,
    pub ksp_version_min: Option<String>,
    pub ksp_version_max: Option<String>,
    pub dependency_edges: usize,
    pub recommendation_edges: usize,
    pub suggestion_edges: usize,
    pub conflict_edges: usize,
    pub provided_identifiers: usize,
}

impl From<&ParsedModule> for ModuleSummary {
    fn from(module: &ParsedModule) -> Self {
        Self {
            path: module.path.clone(),
            identifier: module.identifier.clone(),
            name: module.name.clone(),
            version: module.version.clone(),
            spec_version: module.spec_version.clone(),
            has_abstract: has_text(&module.abstract_text),
            author_count: module.author_count,
            license_count: module.license_count,
            resource_count: module.resource_count,
            install_steps: module.install_steps,
            has_download: module.has_download,
            ksp_version: module.ksp_version.clone(),
            ksp_version_min: module.ksp_version_min.clone(),
            ksp_version_max: module.ksp_version_max.clone(),
            dependency_edges: module.dependency_edges,
            recommendation_edges: module.recommendation_edges,
            suggestion_edges: module.suggestion_edges,
            conflict_edges: module.conflict_edges,
            provided_identifiers: module.provided_identifiers,
        }
    }
}

pub fn has_text(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.is_empty())
}

pub fn clean_string(value: String) -> String {
    value.trim().to_string()
}

pub fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn collection_len(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Array(items)) => items.len(),
        Some(Value::Object(items)) => items.len(),
        Some(Value::Null) | None => 0,
        Some(_) => 1,
    }
}

pub fn has_value(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}
