#!/usr/bin/env bash
#
# Build xsay release binaries for all GPU backend variants supported on the
# current host. Output goes to dist/ with clear variant suffixes so you can
# pick the right one to ship or test.
#
# Default: build only the CPU variant (works everywhere, zero deps).
# Pass variant names to build additional GPU variants:
#
#   ./build.sh                   # CPU only
#   ./build.sh cpu vulkan        # CPU + Vulkan
#   ./build.sh cuda              # CUDA only (NVIDIA + CUDA toolkit)
#   ./build.sh all               # all variants (needs all toolchains)
#
# Requirements per variant (runtime + build):
#   cpu      — nothing extra
#   vulkan   — libvulkan-dev (build), vulkan loader + driver (runtime)
#   cuda     — CUDA toolkit incl. nvcc (build+runtime)
#   hipblas  — ROCm toolkit incl. hipcc (build+runtime)
#   metal    — macOS, automatic

set -euo pipefail

cd "$(dirname "$0")"

DIST_DIR="dist"
TARGET_DIR="target/release"
BIN_NAME="xsay"

mkdir -p "$DIST_DIR"

# Parse variants. No args → CPU only. `all` → every possible variant.
if [ $# -eq 0 ]; then
    VARIANTS=(cpu)
elif [ "$1" = "all" ]; then
    VARIANTS=(cpu vulkan cuda hipblas)
else
    VARIANTS=("$@")
fi

ARCH="$(uname -m)"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
SUFFIX="${OS}-${ARCH}"

build_variant() {
    local variant="$1"
    local features_arg=""
    local label

    case "$variant" in
        cpu)
            features_arg=""
            label="cpu"
            ;;
        vulkan|cuda|hipblas|metal|coreml)
            features_arg="--features $variant"
            label="$variant"
            ;;
        *)
            echo "unknown variant: $variant (expected: cpu, vulkan, cuda, hipblas, metal, coreml)" >&2
            exit 1
            ;;
    esac

    echo "==> building xsay-${label}-${SUFFIX}"
    # shellcheck disable=SC2086  # intentional word-split on features_arg
    cargo build --release $features_arg

    local out="${DIST_DIR}/${BIN_NAME}-${label}-${SUFFIX}"
    cp "${TARGET_DIR}/${BIN_NAME}" "$out"
    strip "$out" 2>/dev/null || true
    local size
    size="$(du -h "$out" | cut -f1)"
    echo "    → $out ($size)"
}

for v in "${VARIANTS[@]}"; do
    build_variant "$v"
done

echo ""
echo "done — binaries in $DIST_DIR/"
ls -lh "$DIST_DIR"/ 2>/dev/null || true
