# Command Reference

## Source Inputs

Most commands accept any of:

- CKAN-meta `.zip`
- CKAN-meta `.tar.gz` or `.tgz`
- Extracted metadata directory

## Pipeline Commands

```bash
ckan-meta-rs fetch --output data/CKAN-meta-master.zip
ckan-meta-rs cache data/CKAN-meta-master.zip data/CKAN-meta-cache --clean
ckan-meta-rs sync --archive data/CKAN-meta-master.zip --cache-dir data/CKAN-meta-cache --export data/modules.jsonl --json-lines
```

`sync` is the full pipeline: download, extract relevant metadata, and optionally
export the bridge file.

## Verification

```bash
scripts/smoke.sh
```

The smoke script runs formatting, tests, Clippy, release build, and fixture-based
CLI checks when CKAN-Linux test fixtures are available.

## Export Commands

```bash
ckan-meta-rs export data/CKAN-meta-cache --output data/summary.json
ckan-meta-rs export data/CKAN-meta-cache --output data/modules.jsonl --json-lines
ckan-meta-rs catalog-index data/CKAN-meta-cache --output data/catalog-index.json --latest-only
ckan-meta-rs validate-catalog-index data/catalog-index.json
ckan-meta-rs validate-export data/summary.json
ckan-meta-rs validate-export data/modules.jsonl --json-lines
```

Package JSON includes schema and aggregate report data. JSON-lines output is one
module summary per line.

`catalog-index` writes the richer optional CKAN-Linux sidecar shape: module
versions, normalized release status, latest flags for stable/testing/development
tolerances, version counts, split relationship target names, reverse relationship
edges, download counts, and provider mappings. It is intended for browse/search
acceleration, not resolver, install, update, or registry-write replacement.
CKAN-Linux falls back to its normal CKAN registry/repository cache path when no
valid sidecar index is configured.

Use `--latest-only` for the smaller browse/search sidecar. Schema v2 retains the
union of the latest candidates for stable, testing, and development tolerances,
allowing a consumer to honor CKAN's instance-wide and per-mod stability settings.
Omit it when a consumer needs every historical module version. JSON is compact
by default; use `--pretty` for manual inspection.

### CKAN-Linux sidecar workflow

For a configured CKAN-Linux development checkout, refresh the live metadata and
atomically replace the default sidecar in one command:

```bash
scripts/refresh-ckan-linux-sidecar.sh
```

The script downloads and extracts current CKAN-meta, builds a latest-only index,
validates it, and only then replaces `data/catalog-index-latest.json`. Override
its paths with `CKAN_META_ARCHIVE`, `CKAN_META_CACHE_DIR`, and
`CKAN_CATALOG_INDEX_OUTPUT`.

Point CKAN-Linux at a generated sidecar with an environment variable:

```bash
CKAN_CATALOG_INDEX_PATH=/path/to/catalog-index.json ckan-linux
```

Alternatively, link it at the default app-data location:

```bash
mkdir -p ~/.local/share/CKAN
ln -s /path/to/catalog-index.json ~/.local/share/CKAN/catalog-index-latest.json
```

## Analysis Commands

```bash
ckan-meta-rs parse data/CKAN-meta-cache
ckan-meta-rs bench data/CKAN-meta-cache --runs 20 --warmups 3
ckan-meta-rs compare data/CKAN-meta-master.zip data/CKAN-meta-cache
ckan-meta-rs latest data/CKAN-meta-cache --limit 20
ckan-meta-rs find data/CKAN-meta-cache Astronomer --limit 20
ckan-meta-rs inspect data/CKAN-meta-cache AstronomersVisualPack --latest --reverse-limit 20
ckan-meta-rs relations data/CKAN-meta-cache TUFX --limit 20
ckan-meta-rs relation-stats data/CKAN-meta-cache --limit 20
ckan-meta-rs unresolved data/CKAN-meta-cache --relationship depends --limit 20
```

## Shell Completions

```bash
ckan-meta-rs completions bash > ckan-meta-rs.bash
ckan-meta-rs completions zsh > _ckan-meta-rs
ckan-meta-rs completions fish > ckan-meta-rs.fish
```
