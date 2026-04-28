use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use rayon::prelude::*;
use tar::Archive;
use zip::ZipArchive;

use crate::model::ExtractionReport;

#[derive(Debug, Clone)]
pub struct TextEntry {
    pub path: String,
    pub contents: String,
}

#[derive(Debug)]
pub struct ArchiveLoad {
    pub archive_entries: usize,
    pub entries: Vec<TextEntry>,
    pub bytes_read: u64,
    pub elapsed: Duration,
}

#[derive(Debug)]
struct DirectoryTextFile {
    path: std::path::PathBuf,
    relative: String,
    len: u64,
}

pub fn archive_kind(path: &Path) -> Result<&'static str> {
    if path.is_dir() {
        return Ok("directory");
    }

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

pub fn load_archive(path: &Path, kind: &str) -> Result<ArchiveLoad> {
    match kind {
        "zip" => load_zip(path),
        "tar.gz" => load_tar_gz(path),
        "directory" => load_directory(path),
        _ => bail!("unsupported archive type: {kind}"),
    }
}

pub fn extract_relevant_entries(
    source: &Path,
    destination: &Path,
    clean: bool,
) -> Result<ExtractionReport> {
    if clean && destination.exists() {
        fs::remove_dir_all(destination)
            .with_context(|| format!("failed to clean {}", destination.display()))?;
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    let started = Instant::now();
    let kind = archive_kind(source)?;
    let mut report = match kind {
        "zip" => extract_zip(source, destination)?,
        "tar.gz" => extract_tar_gz(source, destination)?,
        "directory" => extract_directory(source, destination)?,
        _ => bail!("unsupported archive type: {kind}"),
    };

    report.source = source.display().to_string();
    report.destination = destination.display().to_string();
    report.archive_kind = kind.to_string();
    report.elapsed_ms = started.elapsed().as_millis();

    Ok(report)
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

fn extract_zip(source: &Path, destination: &Path) -> Result<ExtractionReport> {
    let file =
        File::open(source).with_context(|| format!("failed to open {}", source.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive {}", source.display()))?;
    let archive_entries = archive.len();
    let mut relevant_entries = 0;
    let mut bytes_written = 0;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }

        let name = file.name().to_string();
        if !is_relevant_entry(&name) {
            continue;
        }

        let destination_path = safe_destination(destination, &name)?;
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = File::create(&destination_path)?;
        bytes_written += io::copy(&mut file, &mut output)?;
        relevant_entries += 1;
    }

    Ok(ExtractionReport {
        source: String::new(),
        destination: String::new(),
        archive_kind: String::new(),
        archive_entries,
        relevant_entries,
        bytes_written,
        elapsed_ms: 0,
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

fn extract_tar_gz(source: &Path, destination: &Path) -> Result<ExtractionReport> {
    let file =
        File::open(source).with_context(|| format!("failed to open {}", source.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut archive_entries = 0;
    let mut relevant_entries = 0;
    let mut bytes_written = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        archive_entries += 1;
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let path = entry.path()?.to_string_lossy().to_string();
        if !is_relevant_entry(&path) {
            continue;
        }

        let destination_path = safe_destination(destination, &path)?;
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = File::create(&destination_path)?;
        bytes_written += io::copy(&mut entry, &mut output)?;
        relevant_entries += 1;
    }

    Ok(ExtractionReport {
        source: String::new(),
        destination: String::new(),
        archive_kind: String::new(),
        archive_entries,
        relevant_entries,
        bytes_written,
        elapsed_ms: 0,
    })
}

fn load_directory(path: &Path) -> Result<ArchiveLoad> {
    let started = Instant::now();
    let mut archive_entries = 0;
    let mut relevant_files = Vec::new();

    collect_directory_entries(path, path, &mut archive_entries, &mut relevant_files)?;

    let bytes_read = relevant_files.iter().map(|file| file.len).sum();
    let entries = relevant_files
        .par_iter()
        .map(|file| {
            let contents = fs::read_to_string(&file.path)
                .with_context(|| format!("failed to read {} as UTF-8 text", file.path.display()))?;
            Ok(TextEntry {
                path: file.relative.clone(),
                contents,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ArchiveLoad {
        archive_entries,
        entries,
        bytes_read,
        elapsed: started.elapsed(),
    })
}

fn extract_directory(source: &Path, destination: &Path) -> Result<ExtractionReport> {
    let mut archive_entries = 0;
    let mut relevant_files = Vec::new();
    collect_directory_entries(source, source, &mut archive_entries, &mut relevant_files)?;

    let mut relevant_entries = 0;
    let mut bytes_written = 0;
    for file in relevant_files {
        let destination_path = safe_destination(destination, &file.relative)?;
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&file.path, &destination_path)?;
        bytes_written += file.len;
        relevant_entries += 1;
    }

    Ok(ExtractionReport {
        source: String::new(),
        destination: String::new(),
        archive_kind: String::new(),
        archive_entries,
        relevant_entries,
        bytes_written,
        elapsed_ms: 0,
    })
}

fn collect_directory_entries(
    root: &Path,
    current: &Path,
    archive_entries: &mut usize,
    relevant_files: &mut Vec<DirectoryTextFile>,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            collect_directory_entries(root, &path, archive_entries, relevant_files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        *archive_entries += 1;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if !is_relevant_entry(&relative) {
            continue;
        }

        relevant_files.push(DirectoryTextFile {
            path,
            relative,
            len: metadata.len(),
        });
    }

    Ok(())
}

pub fn is_relevant_entry(path: &str) -> bool {
    path.ends_with(".ckan")
        || path.ends_with("download_counts.json")
        || path.ends_with("builds.json")
        || path.ends_with("repositories.json")
}

fn safe_destination(root: &Path, archive_path: &str) -> Result<PathBuf> {
    let components = archive_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let mut relative = PathBuf::new();
    for part in components {
        let path = Path::new(part);
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("unsafe archive path: {archive_path}");
        }
        relative.push(path);
    }

    if relative.as_os_str().is_empty() {
        bail!("empty archive path: {archive_path}");
    }

    Ok(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_destination_keeps_normal_archive_paths_under_root() {
        let destination = safe_destination(
            Path::new("/tmp/cache"),
            "CKAN-meta-master/ModuleManager/ModuleManager-1.ckan",
        )
        .expect("normal archive path should be safe");

        assert_eq!(
            destination,
            Path::new("/tmp/cache/CKAN-meta-master/ModuleManager/ModuleManager-1.ckan")
        );
    }

    #[test]
    fn safe_destination_rejects_parent_components() {
        let result = safe_destination(Path::new("/tmp/cache"), "CKAN-meta-master/../evil.ckan");

        assert!(result.is_err());
    }

    #[test]
    fn safe_destination_rejects_empty_paths() {
        let result = safe_destination(Path::new("/tmp/cache"), "/");

        assert!(result.is_err());
    }
}
