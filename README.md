# ckan-meta-rs

Experimental Rust parser/benchmark tool for CKAN metadata repository archives.

The first goal is to measure whether Rust can parse CKAN metadata archives
meaningfully faster than CKAN's current C# `RepositoryData.FromStream(...)`
path. This project should stay read-only until its output is proven compatible.

## Current MVP

- Accepts a CKAN metadata `.zip` or `.tar.gz` archive.
- Counts archive entries, relevant metadata entries, and `.ckan` files.
- Parses module fields in parallel:
  - `identifier`
  - `name`
  - `version`
  - `spec_version`
  - `abstract`
  - `author`
  - `license`
  - `install`
  - `resources`
  - `download`
  - `ksp_version`, `ksp_version_min`, `ksp_version_max`
- Counts unique identifiers, duplicate identifiers, missing identifiers, and parse errors.
- Counts resolver-relevant relationship buckets:
  - `depends`
  - `recommends`
  - `suggests`
  - `conflicts`
  - `provides`
- Detects `download_counts.json`, `builds.json`, and `repositories.json` when present.
- Reports read/parse/total timings.
- Benchmarks repeated parses with warmups and min/avg/max/total timing stats.
- Emits stable per-module summaries for compatibility comparison work.
- Can emit either a terminal report or JSON.

## Usage

```bash
ckan-meta-rs parse /path/to/CKAN-meta-master.zip
ckan-meta-rs parse /path/to/CKAN-meta-master.tar.gz
ckan-meta-rs parse /path/to/CKAN-meta-master.zip --json
ckan-meta-rs bench /path/to/CKAN-meta-master.zip --runs 20 --warmups 3
ckan-meta-rs bench /path/to/CKAN-meta-master.zip --json
ckan-meta-rs modules /path/to/CKAN-meta-master.zip --limit 20
ckan-meta-rs modules /path/to/CKAN-meta-master.zip --json-lines
```

During development:

```bash
cargo run -- parse /path/to/CKAN-meta-master.zip
cargo run -- bench /path/to/CKAN-meta-master.zip --runs 20 --warmups 3
cargo run -- modules /path/to/CKAN-meta-master.zip --limit 20
cargo test
```

Fetch and benchmark the live metadata repository:

```bash
scripts/fetch-live-meta.sh
cargo build --release
target/release/ckan-meta-rs bench data/CKAN-meta-master.zip --runs 20 --warmups 3
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
Relationship edges: depends=46 recommends=32 suggests=1 conflicts=6 provides=6
Field coverage: abstract=54 author=43 license=54 install=46 resources=49 download=54
KSP compatibility fields: exact=51 min=2 max=0
Parse errors: 0
Special files: download_counts=- builds=- repositories=-
Timing: read=3ms parse=1ms total=4ms
```

Representative live metadata result from a local release build:

```text
Archive: data/CKAN-meta-master.zip
Archive entries: 35314
CKAN metadata entries: 29858
Parsed modules: 29858
Unique identifiers: 3497
Parse errors: 0
Timing statistics:
  read  min=447ms avg=456.25ms max=465ms total=9125ms
  parse min=30ms avg=32.30ms max=35ms total=646ms
  total min=487ms avg=494.85ms max=506ms total=9897ms
```

## Layout

- `src/main.rs`: CLI command wiring.
- `src/archive.rs`: archive type detection and zip/tar.gz text loading.
- `src/model.rs`: serializable reports and module summaries.
- `src/parser.rs`: CKAN JSON parsing, report construction, and benchmark loops.
- `src/output.rs`: terminal, JSON, and JSON-lines output formatting.

## Scope

This tool is intentionally read-only. It does not install mods, write CKAN
registries, or claim CKAN-compatible semantics yet. The first useful benchmark
is raw archive and JSON throughput compared with CKAN's existing
`RepositoryData.FromStream(...)` path.

## Next Steps

- Compare the parsed counts against CKAN's `RepositoryData.FromStream(...)` output.
- Investigate whether zip decompression or archive layout is the main read-time bottleneck.
- Only consider CKAN integration if the parser is substantially faster on real metadata.
