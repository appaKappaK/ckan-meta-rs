# ckan-meta-rs

Experimental Rust parser/benchmark tool for CKAN metadata repository archives.

The first goal is to measure whether Rust can parse CKAN metadata archives
meaningfully faster than CKAN's current C# `RepositoryData.FromStream(...)`
path. This project should stay read-only until its output is proven compatible.

## Current MVP

- Accepts a CKAN metadata `.zip`, `.tar.gz`, or extracted directory.
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
ckan-meta-rs parse /path/to/extracted/CKAN-meta-master
ckan-meta-rs parse /path/to/CKAN-meta-master.zip --json
ckan-meta-rs bench /path/to/CKAN-meta-master.zip --runs 20 --warmups 3
ckan-meta-rs bench /path/to/CKAN-meta-master.zip --json
ckan-meta-rs modules /path/to/CKAN-meta-master.zip --limit 20
ckan-meta-rs modules /path/to/CKAN-meta-master.zip --json-lines
ckan-meta-rs find /path/to/CKAN-meta-master.zip AVP-4kTextures --json-lines
ckan-meta-rs relations /path/to/CKAN-meta-master.zip AstronomersVisualPack
ckan-meta-rs compare /path/to/CKAN-meta-master.zip /path/to/extracted/CKAN-meta-master
```

During development:

```bash
cargo run -- parse /path/to/CKAN-meta-master.zip
cargo run -- bench /path/to/CKAN-meta-master.zip --runs 20 --warmups 3
cargo run -- modules /path/to/CKAN-meta-master.zip --limit 20
cargo run -- find /path/to/CKAN-meta-master.zip Astronomer --limit 20
cargo run -- relations /path/to/CKAN-meta-master.zip TUFX --limit 20
cargo run -- compare /path/to/CKAN-meta-master.zip /path/to/extracted/CKAN-meta-master
cargo test
```

Fetch and benchmark the live metadata repository:

```bash
scripts/fetch-live-meta.sh
cargo build --release
target/release/ckan-meta-rs bench data/CKAN-meta-master.zip --runs 20 --warmups 3
unzip -q data/CKAN-meta-master.zip -d data
target/release/ckan-meta-rs bench data/CKAN-meta-master --runs 20 --warmups 3
```

Or run the zip and extracted-directory comparison script:

```bash
scripts/bench-live-meta.sh
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
Type: zip
Archive entries: 35314
CKAN metadata entries: 29858
Parsed modules: 29858
Unique identifiers: 3497
Parse errors: 0
Timing statistics:
  read  min=463ms avg=482.60ms max=507ms total=2413ms
  parse min=43ms avg=60.20ms max=76ms total=301ms
  total min=513ms avg=551.40ms max=586ms total=2757ms
```

The same extracted metadata directory avoids zip decompression and central
directory overhead:

```text
Archive: data/CKAN-meta-master
Type: directory
Parsed modules: 29858
Parse errors: 0
Timing statistics:
  read  min=125ms avg=133.90ms max=147ms total=1339ms
  parse min=46ms avg=51.80ms max=63ms total=518ms
  total min=176ms avg=188.80ms max=208ms total=1888ms
```

Per-module JSON output includes relationship names:

```json
{
  "identifier": "AVP-4kTextures",
  "version": "v1.13",
  "dependency_names": ["AstronomersVisualPack"],
  "conflict_names": ["AVP-Textures"],
  "provided_names": ["AVP-Textures"]
}
```

Reverse relationship lookup shows which modules reference a target:

```text
Relation   Target                           Identifier                       Version          KSP
depends    AstronomersVisualPack            AVP-4kTextures                   v1.13            1.8+
recommends TUFX                             AstronomersVisualPack            3:v4.13          1.12.0-1.12.9
```

The `compare` command checks that two sources produce the same metadata counts
and normalized per-module fingerprints:

```text
Left: data/CKAN-meta-master.zip
Right: data/CKAN-meta-master
Matching: true
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
- Investigate whether a persistent unpacked metadata cache is practical for CKAN-Linux.
- Only consider CKAN integration if the parser is substantially faster on real metadata.
