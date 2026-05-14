# Findings

## Current Signal

The CLI parses the live `KSP-CKAN/CKAN-meta` repository successfully:

- CKAN metadata entries: `29,858`
- Unique identifiers: `3,497`
- Parse errors: `0`
- Relationship edges:
  - `depends`: `49,224`
  - `recommends`: `25,120`
  - `suggests`: `31,483`
  - `conflicts`: `11,934`
  - `provides`: `4,195`

Release benchmark results on this machine:

| Source | Read avg | Parse avg | Total avg |
| --- | ---: | ---: | ---: |
| `data/CKAN-meta-master.zip` | `440.50ms` | `39.35ms` | `482.65ms` |
| `data/CKAN-meta-master` extracted directory | `110.85ms` | `44.70ms` | `156.70ms` |

The key result is that JSON parsing is not the main bottleneck. Archive loading,
especially zip reading/decompression, dominates the end-to-end time.

## Practical Implication

The Rust helper should not replace CKAN's zip parsing, repository update,
resolver, install, or registry-write behavior. The useful role is to prepare an
optional browse/search sidecar while CKAN-Linux keeps CKAN core authoritative.
The current optimization path is:

1. Download the CKAN-meta archive normally.
2. Maintain a persistent extracted metadata cache.
3. Scan the extracted cache in parallel.
4. Produce a compact JSON sidecar for CKAN-Linux catalog/search loading.

That approach keeps network and compatibility behavior conservative while
removing repeated archive overhead.

## Relationship Inspection Example

`AVP-4kTextures` declares:

- `depends`: `AstronomersVisualPack`
- `conflicts`: `AVP-Textures`
- `provides`: `AVP-Textures`

`AstronomersVisualPack` declares, for recent versions:

- `depends`: `AVP-Textures`, `EnvironmentalVisualEnhancements`, `ModuleManager`, `Scatterer`
- `recommends`: `TUFX`
- `suggests`: `Chatterer`, `DistantObject`, `PlanetShine`
- `conflicts`: `EnvironmentalVisualEnhancements-Config`
- `provides`: `EnvironmentalVisualEnhancements-Config`, `EnvironmentalVisualEnhancements-Config-stock`

Useful commands:

```bash
target/release/ckan-meta-rs find data/CKAN-meta-master AVP-4kTextures --json-lines --limit 4
target/release/ckan-meta-rs relations data/CKAN-meta-master AstronomersVisualPack --limit 20
target/release/ckan-meta-rs relations data/CKAN-meta-master TUFX --limit 20
target/release/ckan-meta-rs catalog-index data/CKAN-meta-master --output data/catalog-index.json --latest-only
```

## Current CKAN-Linux Integration

- `catalog-index` is the optional CKAN-Linux sidecar contract for catalog/search acceleration.
- CKAN-Linux consumes the sidecar only when a valid index is configured.
- CKAN-Linux falls back to the normal CKAN registry/repository cache path when the sidecar is missing or invalid.
- CKAN core remains authoritative for metadata details, installs, updates, dependency resolution, compatibility decisions, and registry writes.
- Keep comparing generator timings here with CKAN-Linux consumer timings from `scripts/benchmark-linuxgui-catalog.sh` in the CKAN-Linux repo.
