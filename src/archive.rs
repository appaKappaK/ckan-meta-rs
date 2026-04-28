use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use rayon::prelude::*;
use tar::Archive;
use zip::ZipArchive;

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
