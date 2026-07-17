#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
archive_path="${CKAN_META_ARCHIVE:-$repo_root/data/CKAN-meta-master.zip}"
cache_dir="${CKAN_META_CACHE_DIR:-$repo_root/data/CKAN-meta-cache}"
output_path="${CKAN_CATALOG_INDEX_OUTPUT:-$repo_root/data/catalog-index-latest.json}"
output_dir=$(dirname -- "$output_path")

mkdir -p "$output_dir"
temp_output=$(mktemp "$output_dir/.catalog-index-latest.XXXXXX.json")
cleanup() {
    rm -f -- "$temp_output"
}
trap cleanup EXIT

cd "$repo_root"
cargo run --release --locked -- sync \
    --archive "$archive_path" \
    --cache-dir "$cache_dir"
cargo run --release --locked -- catalog-index "$cache_dir" \
    --output "$temp_output" \
    --latest-only
cargo run --release --locked -- validate-catalog-index "$temp_output"

mv -f -- "$temp_output" "$output_path"
trap - EXIT
printf 'Updated CKAN Linux catalog sidecar: %s\n' "$output_path"
