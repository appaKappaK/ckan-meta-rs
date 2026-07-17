# ckan-meta-rs

An experimental Rust CLI for exploring
[CKAN](https://github.com/KSP-CKAN/CKAN) metadata and generating an optional,
fast catalog index for CKAN-Linux.

`ckan-meta-rs` is intentionally read-only. CKAN remains responsible for
repository updates, compatibility, dependency resolution, installs, and
registry changes.

## Highlights

- Parses CKAN-meta archives and extracted directories in parallel.
- Searches modules and relationships, compares sources, and benchmarks parsing.
- Exports module summaries as JSON or JSON Lines.
- Generates and validates the schema-v2 CKAN-Linux catalog sidecar, including
  stable, testing, and development release candidates.

## Getting Started

Requires a stable Rust toolchain.

```bash
cargo build --release
target/release/ckan-meta-rs --help
```

Download and cache the current metadata, then inspect it:

```bash
target/release/ckan-meta-rs sync \
  --archive data/CKAN-meta-master.zip \
  --cache-dir data/CKAN-meta-cache

target/release/ckan-meta-rs parse data/CKAN-meta-cache
target/release/ckan-meta-rs find data/CKAN-meta-cache Astronomer --limit 20
```

Most commands accept a CKAN-meta `.zip`, `.tar.gz`/`.tgz`, or extracted
directory.

## CKAN-Linux Sidecar

To refresh, build, validate, and atomically replace the local sidecar:

```bash
scripts/refresh-ckan-linux-sidecar.sh
```

The sidecar only accelerates catalog browsing and search. CKAN-Linux falls back
to its normal registry cache whenever a valid sidecar is unavailable.

See the [command reference](docs/commands.md#export-commands) for manual
generation, configuration, and schema details.

## Documentation

- [Command reference and workflows](docs/commands.md)
- [Benchmarks and CKAN-Linux integration findings](docs/findings.md)

## Development

Run the full local verification suite with:

```bash
scripts/smoke.sh
```
