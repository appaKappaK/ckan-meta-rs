#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)

usage() {
    cat <<'EOF'
Build and install ckan-meta-rs from the current checkout.

Usage: scripts/install.sh [--install-dir DIR]

Options:
  --install-dir DIR  Install the binary into DIR
  -h, --help         Show this help

The install directory defaults to CKAN_META_RS_INSTALL_DIR, then XDG_BIN_HOME,
then ~/.local/bin.
EOF
}

install_dir="${CKAN_META_RS_INSTALL_DIR:-}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-dir)
            if [[ $# -lt 2 || -z "$2" ]]; then
                printf 'error: --install-dir requires a directory\n' >&2
                exit 2
            fi
            install_dir="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'error: unknown option: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "$install_dir" && -n "${XDG_BIN_HOME:-}" ]]; then
    install_dir="$XDG_BIN_HOME"
elif [[ -z "$install_dir" && -n "${HOME:-}" ]]; then
    install_dir="$HOME/.local/bin"
elif [[ -z "$install_dir" ]]; then
    printf 'error: HOME is unset; pass --install-dir or set CKAN_META_RS_INSTALL_DIR\n' >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    printf 'error: cargo was not found; install a stable Rust toolchain first\n' >&2
    exit 1
fi

printf 'Building ckan-meta-rs in release mode...\n'
cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    --target-dir "$repo_root/target" \
    --release \
    --locked

mkdir -p -- "$install_dir"
temp_binary=$(mktemp "$install_dir/.ckan-meta-rs.XXXXXX")
cleanup() {
    rm -f -- "$temp_binary"
}
trap cleanup EXIT

install -m 0755 "$repo_root/target/release/ckan-meta-rs" "$temp_binary"
"$temp_binary" --help >/dev/null
mv -f -- "$temp_binary" "$install_dir/ckan-meta-rs"
trap - EXIT

printf 'Installed ckan-meta-rs to %s\n' "$install_dir/ckan-meta-rs"
printf 'CKAN-Linux can now refresh its Rust catalog sidecar automatically.\n'
case ":${PATH:-}:" in
    *":$install_dir:"*) ;;
    *) printf 'Add %s to PATH to run ckan-meta-rs from any directory.\n' "$install_dir" ;;
esac
