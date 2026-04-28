#!/usr/bin/env bash
set -euo pipefail

zip_path="${1:-data/CKAN-meta-master.zip}"
dir_path="${2:-data/CKAN-meta-master}"
runs="${RUNS:-10}"
warmups="${WARMUPS:-2}"

if [[ ! -f "$zip_path" ]]; then
    scripts/fetch-live-meta.sh "https://github.com/KSP-CKAN/CKAN-meta/archive/refs/heads/master.zip" "$zip_path"
fi

rm -rf "$dir_path"
unzip -q "$zip_path" -d "$(dirname "$dir_path")"

cargo build --release

target/release/ckan-meta-rs bench "$zip_path" --runs "$runs" --warmups "$warmups"
echo
target/release/ckan-meta-rs bench "$dir_path" --runs "$runs" --warmups "$warmups"
