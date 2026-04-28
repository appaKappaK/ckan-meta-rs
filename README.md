# ckan-meta-rs

Experimental Rust parser/benchmark tool for CKAN metadata repository archives.

The first goal is to measure whether Rust can parse CKAN metadata archives
meaningfully faster than CKAN's current C# `RepositoryData.FromStream(...)`
path. This project should stay read-only until its output is proven compatible.

## Current MVP

- Accepts a CKAN metadata `.zip` or `.tar.gz` archive.
- Counts archive entries, relevant metadata entries, and `.ckan` files.
- Parses minimal module fields in parallel:
  - `identifier`
  - `name`
  - `version`
  - `spec_version`
- Counts unique identifiers, duplicate identifiers, missing identifiers, and parse errors.
- Detects `download_counts.json`, `builds.json`, and `repositories.json` when present.
- Reports read/parse/total timings.
- Can emit either a terminal report or JSON.

## Usage

```bash
ckan-meta-rs parse /path/to/CKAN-meta-master.zip
ckan-meta-rs parse /path/to/CKAN-meta-master.tar.gz
ckan-meta-rs parse /path/to/CKAN-meta-master.zip --json
```

During development:

```bash
cargo run -- parse /path/to/CKAN-meta-master.zip
cargo test
```

Example output:

```text
Archive: CKAN-meta-testkan.zip
Type: zip
Archive entries: 60
Relevant entries: 54
CKAN metadata entries: 54
Parsed modules: 54
Named modules: 54
Versioned modules: 54
Spec-versioned modules: 54
Unique identifiers: 41
Duplicate identifiers: 10
Missing identifiers: 0
Parse errors: 0
Special files: download_counts=- builds=- repositories=-
Timing: read=3ms parse=1ms total=4ms
```

## Scope

This tool is intentionally read-only. It does not install mods, write CKAN
registries, or claim CKAN-compatible semantics yet. The first useful benchmark
is raw archive and JSON throughput compared with CKAN's existing
`RepositoryData.FromStream(...)` path.

## Next Steps

- Add a benchmark command that runs the same archive repeatedly.
- Parse relationship fields enough to estimate resolver-relevant metadata size.
- Download or point at a live CKAN-meta archive and compare debug vs release timings.
- Only consider CKAN integration if the parser is substantially faster on real metadata.
