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

is_loader_or_kernel_pseudo_lib() {
    case "$1" in
        linux-vdso.so.*|ld-linux-*.so.*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

declare -A seen_inputs=()
declare -A copied=()
queue=("$BINARY")

copy_runtime_deps() {
    local input="$1"
    while IFS=$'\t' read -r soname path; do
        [[ -n "${soname:-}" && -n "${path:-}" ]] || continue
        [[ "$path" == /* ]] || continue
        if is_glibc_runtime "$soname" || is_loader_or_kernel_pseudo_lib "$soname"; then
            continue
        fi

        local dest_path="$DEST/$soname"
        if [[ -z "${copied[$soname]:-}" ]]; then
            cp -L "$path" "$dest_path"
            copied[$soname]=1
        fi

        # GTK/AppIndicator/X11 stacks pull in many second-level shared
        # libraries. Walk copied ELF files too so the .deb can be used
        # offline with the private /usr/lib/xsay runtime directory.
        if [[ -z "${seen_inputs[$path]:-}" ]]; then
            queue+=("$path")
        fi
    done < <(
        ldd "$input" |
            awk '
            /=> \// { print $1 "\t" $3; next }
            /^\t\// { n=$1; sub(/^.*\//, "", n); print n "\t" $1 }
        '
    )
}

while ((${#queue[@]} > 0)); do
    input="${queue[0]}"
    queue=("${queue[@]:1}")
    [[ -n "${seen_inputs[$input]:-}" ]] && continue
    seen_inputs[$input]=1
    copy_runtime_deps "$input"
done

# sherpa-rs downloads ONNX runtime / sherpa shared objects into target/release.
# They are usually direct dependencies, but copy them explicitly as a guard for
# linkers that resolve them via rpath at runtime instead of showing them in ldd.
while IFS= read -r lib; do
    soname="$(basename "$lib")"
    [[ -n "${copied[$soname]:-}" ]] && continue
    cp -L "$lib" "$DEST/$soname"
    copied[$soname]=1
done < <(find "$(dirname "$BINARY")" -maxdepth 1 -type f \( \
    -name 'libsherpa*.so*' -o -name 'libonnxruntime*.so*' \
\) -print 2>/dev/null)

count="$(find "$DEST" -maxdepth 1 -type f | wc -l)"
size="$(du -sh "$DEST" | awk '{print $1}')"
echo "vendored $count runtime libraries into $DEST ($size)"
