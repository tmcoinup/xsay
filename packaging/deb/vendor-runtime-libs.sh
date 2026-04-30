#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${1:-$ROOT_DIR/target/release/xsay}"
DEST="${2:-$ROOT_DIR/target/debian-vendor-libs}"

if [[ ! -x "$BINARY" ]]; then
    echo "error: binary not found or not executable: $BINARY" >&2
    echo "hint: run cargo build --release first" >&2
    exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST"

is_glibc_runtime() {
    case "$1" in
        libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|ld-linux-*.so.*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

ldd "$BINARY" |
    awk '
        /=> \// { print $1 "\t" $3; next }
        /^\t\// { n=$1; sub(/^.*\//, "", n); print n "\t" $1 }
    ' |
    while IFS=$'\t' read -r soname path; do
        [[ -n "${soname:-}" && -n "${path:-}" ]] || continue
        [[ "$path" == /* ]] || continue
        if is_glibc_runtime "$soname"; then
            continue
        fi
        cp -L "$path" "$DEST/$soname"
    done

count="$(find "$DEST" -maxdepth 1 -type f | wc -l)"
size="$(du -sh "$DEST" | awk '{print $1}')"
echo "vendored $count runtime libraries into $DEST ($size)"
