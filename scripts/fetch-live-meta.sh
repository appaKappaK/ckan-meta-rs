#!/usr/bin/env bash
set -euo pipefail

repo_url="${1:-https://github.com/KSP-CKAN/CKAN-meta/archive/refs/heads/master.zip}"
output="${2:-data/CKAN-meta-master.zip}"

mkdir -p "$(dirname "$output")"
curl -L --fail --show-error --progress-bar "$repo_url" -o "$output"
ls -lh "$output"
