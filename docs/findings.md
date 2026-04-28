# Findings

## Current Signal

The prototype can parse the live `KSP-CKAN/CKAN-meta` repository successfully:

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
| `data/CKAN-meta-master.zip` | `482.60ms` | `60.20ms` | `551.40ms` |
| `data/CKAN-meta-master` extracted directory | `133.90ms` | `51.80ms` | `188.80ms` |

The key result is that JSON parsing is not the main bottleneck. Archive loading,
especially zip reading/decompression, dominates the end-to-end time.

## Practical Implication

A Rust helper probably should not just replace CKAN's zip parsing one-for-one.
The stronger optimization path is:

1. Download the CKAN-meta archive normally.
2. Maintain a persistent extracted metadata cache.
3. Scan the extracted cache in parallel.
4. Produce a compact JSON or binary summary for the GUI or C# layer.

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
```

## Next Technical Steps

- Compare this summary output against CKAN's `RepositoryData.FromStream(...)`.
- Decide whether the helper should output JSON lines, a single JSON summary, or a compact binary cache.
- Measure whether CKAN-Linux can use an extracted metadata cache without disrupting existing repository update semantics.
