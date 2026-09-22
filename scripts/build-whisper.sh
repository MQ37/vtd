#!/usr/bin/env bash
# Clones and builds whisper.cpp for use by vtd, with GPU (ROCm/HIP) acceleration
# when available, falling back to a CPU-only build otherwise.
#
# Usage:
#   scripts/build-whisper.sh          # auto-detect GPU, fall back to CPU
#   scripts/build-whisper.sh --cpu    # force a CPU-only build
#
# Respects (all optional):
#   XDG_DATA_HOME     where whisper.cpp is cloned/built (default ~/.local/share)
#   ROCM_PATH         ROCm install root (default /opt/rocm)
#   VTD_HIP_COMPILER  override the HIP C++ compiler path if auto-detection fails
#   VTD_GPU_ARCH      explicit CMAKE_HIP_ARCHITECTURES / AMDGPU_TARGETS
#                     (e.g. gfx1151 for Strix Halo); left to cmake to detect if unset

set -euo pipefail

FORCE_CPU=0
if [[ "${1:-}" == "--cpu" ]]; then
    FORCE_CPU=1
fi

DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/vtd"
WHISPER_DIR="$DATA_DIR/whisper.cpp"
PATCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/patches"

mkdir -p "$DATA_DIR"

if [[ ! -d "$WHISPER_DIR" ]]; then
    echo "==> Cloning whisper.cpp into $WHISPER_DIR"
    git clone --depth 1 https://github.com/ggml-org/whisper.cpp.git "$WHISPER_DIR"
fi

cd "$WHISPER_DIR"

for patch in "$PATCH_DIR"/*.patch; do
    [[ -e "$patch" ]] || continue
    if git apply --check --reverse "$patch" 2>/dev/null; then
        echo "==> Patch already applied: $(basename "$patch")"
    elif git apply "$patch" 2>/dev/null; then
        echo "==> Applied patch: $(basename "$patch")"
    else
        echo "==> WARNING: could not apply $(basename "$patch") (may not be needed for your ROCm version)"
    fi
done

ROCM_PATH="${ROCM_PATH:-/opt/rocm}"
HAVE_HIP=0
if [[ $FORCE_CPU -eq 0 ]] && { command -v hipcc >/dev/null 2>&1 || [[ -x "$ROCM_PATH/bin/hipcc" ]]; }; then
    HAVE_HIP=1
fi

rm -rf build

if [[ $HAVE_HIP -eq 1 ]]; then
    echo "==> ROCm/HIP detected, attempting GPU-accelerated build (ROCM_PATH=$ROCM_PATH)"
    CMAKE_ARGS=(-B build -DGGML_HIP=ON -DCMAKE_BUILD_TYPE=Release
        "-DCMAKE_HIP_FLAGS=-isystem $ROCM_PATH/include")
    [[ -n "${VTD_HIP_COMPILER:-}" ]] && CMAKE_ARGS+=("-DCMAKE_HIP_COMPILER=$VTD_HIP_COMPILER")
    [[ -n "${VTD_GPU_ARCH:-}" ]] && CMAKE_ARGS+=("-DCMAKE_HIP_ARCHITECTURES=$VTD_GPU_ARCH" "-DAMDGPU_TARGETS=$VTD_GPU_ARCH")

    if ROCM_PATH="$ROCM_PATH" HIP_PATH="$ROCM_PATH" cmake "${CMAKE_ARGS[@]}" \
        && cmake --build build --config Release -j"$(nproc)"; then
        echo "==> GPU build succeeded: $WHISPER_DIR/build/bin/whisper-cli"
        exit 0
    fi
    echo "==> GPU build failed, falling back to CPU-only build"
    rm -rf build
fi

echo "==> Building CPU-only"
cmake -B build -DGGML_HIP=OFF -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release -j"$(nproc)"
echo "==> CPU build succeeded: $WHISPER_DIR/build/bin/whisper-cli"
