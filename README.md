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
- Builds that sidecar directly from CKAN's parsed repository cache, including
  the active custom repositories selected by CKAN-Linux.

## Getting Started

Requires a stable Rust toolchain. Install the current checkout to
`$XDG_BIN_HOME`, when set, or `~/.local/bin` with:

```bash
scripts/install.sh
ckan-meta-rs --help
```

Download and cache the current metadata, then inspect it:

```bash
ckan-meta-rs sync \
  --archive data/CKAN-meta-master.zip \
  --cache-dir data/CKAN-meta-cache

ckan-meta-rs parse data/CKAN-meta-cache
ckan-meta-rs find data/CKAN-meta-cache Astronomer --limit 20
```

Most commands accept a CKAN-meta `.zip`, `.tar.gz`/`.tgz`, or extracted
directory.

## CKAN-Linux Sidecar

Once the binary is installed, current CKAN-Linux builds discover it and refresh
the fast catalog automatically after CKAN updates its repository metadata. The
helper consumes the exact cache files CKAN just loaded, so no second metadata
download is needed. A missing or failed helper is nonfatal; CKAN-Linux falls
back to its normal registry catalog.

The equivalent one-shot command is:

```bash
ckan-meta-rs refresh-sidecar \
  --repository-cache ~/.local/share/CKAN/repos/HASH-KSP-default.json \
  --output ~/.local/share/CKAN/catalog-index-latest.json
```

Repeat `--repository-cache` in CKAN priority order when more than one repository
is active. The command builds and validates a sibling temporary file, then
atomically replaces the output only after the new index is valid.

The source-checkout workflow remains available for generating a default-repo
sidecar without CKAN's parsed cache:

```bash
scripts/refresh-ckan-linux-sidecar.sh
```

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
