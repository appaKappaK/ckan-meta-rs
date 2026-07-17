# ckan-meta-rs

Experimental Rust CLI for reading CKAN metadata repositories, benchmarking parse
performance, and producing optional catalog/search sidecar files for CKAN-Linux.

The project is intentionally read-only. It does not replace CKAN's resolver,
repository update path, install logic, or registry writes; it parses metadata
into comparison and sidecar outputs that CKAN-Linux can consume as an optional
browse/search acceleration layer.

## What It Does

- Reads CKAN metadata from `.zip`, `.tar.gz`/`.tgz`, or extracted directories.
- Parses `.ckan` modules in parallel.
- Reports archive, metadata, field coverage, relationship, and timing counts.
- Emits per-module summaries as terminal tables, JSON arrays, or JSON lines.
- Selects the latest version per identifier using CKAN-ish version ordering.
- Exports stable package summaries for compatibility comparison.
- Builds an optional CKAN-Linux catalog/search sidecar index with module rows,
  release-stability candidates, reverse relationship edges, download counts,
  and provider mappings.
- Downloads live CKAN-meta archives and maintains extracted metadata caches.
- Searches identifiers, names, versions, relationship targets, unresolved
  references, and reverse relationships.
- Compares two metadata sources by normalized per-module fingerprints.

Parsed fields currently include identifiers, names, versions, spec versions,
abstracts/descriptions, authors, licenses, kinds, release dates, download sizes,
download resources, install stanzas, KSP compatibility fields, and resolver
relationship buckets (`depends`, `recommends`, `suggests`, `conflicts`,
`provides`).

## Install

Requirements:

- Rust stable toolchain
- Network access only for `fetch`, `sync`, or the live benchmark scripts

Build from the repository:

```bash
cargo build --release
target/release/ckan-meta-rs --help
```

Or run directly during development:

```bash
cargo run -- --help
```

## Quick Start

Download the live CKAN metadata archive, extract a cache, and export JSON lines:

```bash
cargo run -- sync \
  --archive data/CKAN-meta-master.zip \
  --cache-dir data/CKAN-meta-cache \
  --export data/modules.jsonl \
  --json-lines
```

Run common analysis commands against the extracted cache:

```bash
cargo run -- parse data/CKAN-meta-cache
cargo run -- bench data/CKAN-meta-cache --runs 20 --warmups 3
cargo run -- latest data/CKAN-meta-cache --limit 20
cargo run -- find data/CKAN-meta-cache Astronomer --limit 20
cargo run -- inspect data/CKAN-meta-cache AstronomersVisualPack --latest
cargo run -- relation-stats data/CKAN-meta-cache --limit 20
```

Build the optional CKAN-Linux sidecar index:

```bash
cargo run -- catalog-index data/CKAN-meta-cache \
  --output data/catalog-index.json \
  --latest-only \
  --pretty

cargo run -- validate-catalog-index data/catalog-index.json
```

Catalog schema v2 normalizes CKAN `release_status` values and marks the latest
candidate allowed by stable, testing, and development tolerances. With
`--latest-only`, the output retains the small union of those candidates rather
than every historical version.

For a configured CKAN-Linux development checkout, refresh the live metadata and
atomically replace the default sidecar in one command:

```bash
scripts/refresh-ckan-linux-sidecar.sh
```

The script downloads and extracts current CKAN-meta, builds a latest-only index,
validates it, and only then replaces `data/catalog-index-latest.json`. Its archive,
cache, and output paths can be overridden with `CKAN_META_ARCHIVE`,
`CKAN_META_CACHE_DIR`, and `CKAN_CATALOG_INDEX_OUTPUT`.

To use the generated index with CKAN-Linux, either point the app at it:

```bash
CKAN_CATALOG_INDEX_PATH=/path/to/catalog-index.json ckan-linux
```

or symlink it into CKAN app data:

```bash
mkdir -p ~/.local/share/CKAN
ln -s /path/to/catalog-index.json ~/.local/share/CKAN/catalog-index-latest.json
```

CKAN-Linux does not require this repo. If no valid sidecar index is configured,
CKAN-Linux falls back to CKAN's normal registry/repository cache path.

## Commands

```text
parse                   Parse a source and report counts/timing
bench                   Repeated parse benchmark with warmups
modules                 Emit parsed module summaries
latest                  Emit latest module summary per identifier
export                  Write stable package JSON or JSON lines
catalog-index           Write optional CKAN-Linux catalog/search sidecar JSON
validate-catalog-index  Validate a catalog sidecar index
validate-export         Validate an exported summary file
fetch                   Download a CKAN metadata archive
sync                    Download, extract, and optionally export
cache                   Extract relevant metadata into a cache directory
find                    Search by identifier, name, or version
relations               Show modules that reference a relationship target
relation-stats          Count common relationship targets
unresolved              Find missing relationship targets
inspect                 Inspect a module and reverse relationships
compare                 Compare two metadata sources
completions             Generate shell completions
```

Most commands accept a CKAN-meta `.zip`, `.tar.gz`/`.tgz`, or extracted metadata
directory.

See [docs/commands.md](docs/commands.md) for detailed command examples.

## Output Formats

Terminal reports are meant for quick inspection:

```text
Archive: data/CKAN-meta-master.zip
Type: zip
CKAN metadata entries: 29858
Parsed modules: 29858
Unique identifiers: 3497
Parse errors: 0
Timing statistics:
  read  min=432ms avg=440.50ms max=465ms total=8810ms
  parse min=35ms avg=39.35ms max=48ms total=787ms
  total min=470ms avg=482.65ms max=505ms total=9653ms
```

JSON output is available on report-style commands with `--json`. Module lists can
be emitted as pretty JSON arrays with `--json` or newline-delimited JSON with
`--json-lines`.

Example module summary:

```json
{
  "identifier": "AVP-4kTextures",
  "version": "v1.13",
  "dependency_names": ["AstronomersVisualPack"],
  "conflict_names": ["AVP-Textures"],
  "provided_names": ["AVP-Textures"]
}
```

## Development

Run the local verification script:

```bash
scripts/smoke.sh
```

The smoke script runs formatting, tests, Clippy, release build, and fixture-based
CLI checks when CKAN-Linux test fixtures are available.

Equivalent core checks:

```bash
cargo fmt -- --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

Fetch and benchmark the live metadata repository:

```bash
scripts/fetch-live-meta.sh
cargo build --release
target/release/ckan-meta-rs bench data/CKAN-meta-master.zip --runs 20 --warmups 3
unzip -q data/CKAN-meta-master.zip -d data
target/release/ckan-meta-rs bench data/CKAN-meta-master --runs 20 --warmups 3
```

Or run the bundled comparison script:

```bash
scripts/bench-live-meta.sh
```

## Current Findings

On the current live metadata set, JSON parsing is not the main bottleneck. Zip
reading and decompression dominate end-to-end time, while scanning an extracted
cache is substantially faster.

The current sidecar path is:

1. Download CKAN-meta normally.
2. Maintain a persistent extracted metadata cache.
3. Scan the extracted cache in parallel.
4. Produce compact JSON sidecars for optional CKAN-Linux catalog/search loading.

CKAN-Linux consumes the sidecar only when a valid index is configured and falls
back to CKAN's normal registry/repository cache path otherwise.

See [docs/findings.md](docs/findings.md) for benchmark numbers and integration
notes.

## Repository Layout

- [src/](src): CLI, archive readers, parser, output, and export validation code.
- [scripts/](scripts): live metadata fetch, benchmark, and smoke scripts.
- [docs/commands.md](docs/commands.md): command reference and common workflows.
- [docs/findings.md](docs/findings.md): benchmark results and integration notes.
