use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::model::DownloadReport;

pub fn download_to_file(url: &str, output: &Path) -> Result<DownloadReport> {
    let started = Instant::now();
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to download {url}"))?;
    let mut reader = response.into_reader();
    let mut file =
        File::create(output).with_context(|| format!("failed to create {}", output.display()))?;
    let bytes_written = io::copy(&mut reader, &mut file)?;

    Ok(DownloadReport {
        url: url.to_string(),
        output: output.display().to_string(),
        bytes_written,
        elapsed_ms: started.elapsed().as_millis(),
    })
}
