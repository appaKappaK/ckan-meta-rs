#!/usr/bin/env bash
set -euo pipefail

fixture_dir="${FIXTURE_DIR:-/home/matth/GithubProjects/CKAN-github/CKAN-Linux/Tests/Data}"
zip_fixture="$fixture_dir/CKAN-meta-testkan.zip"
tar_fixture="$fixture_dir/CKAN-meta-testkan.tar.gz"

cargo fmt -- --check
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked

if [[ -f "$zip_fixture" ]]; then
    target/release/ckan-meta-rs parse "$zip_fixture" >/dev/null
    target/release/ckan-meta-rs modules "$zip_fixture" --limit 3 >/dev/null
    target/release/ckan-meta-rs latest "$zip_fixture" --limit 3 >/dev/null
    target/release/ckan-meta-rs export "$zip_fixture" --output /tmp/ckan-meta-rs-smoke.json
    target/release/ckan-meta-rs validate-export /tmp/ckan-meta-rs-smoke.json >/dev/null
fi

if [[ -f "$zip_fixture" && -f "$tar_fixture" ]]; then
    target/release/ckan-meta-rs compare "$zip_fixture" "$tar_fixture" >/dev/null
fi

echo "smoke checks passed"
