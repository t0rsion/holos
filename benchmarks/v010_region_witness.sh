#!/usr/bin/env bash
# Generate the registered strict-containment witness record.
set -euo pipefail

CARGO="${CARGO:-cargo +1.92}"
BUILD_CPUS="${BUILD_CPUS:-16-31}"
OUTPUT="${OUTPUT:-benchmarks/results_v010_region_witness.md}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" && "${ALLOW_DIRTY:-}" != "1" ]]; then
    echo "error: commit the worktree before generating the registered witness, or set ALLOW_DIRTY=1" >&2
    exit 1
fi
if ! command -v taskset >/dev/null 2>&1; then
    echo "error: taskset is required to pin the witness build" >&2
    exit 1
fi

read -r -a cargo_command <<<"$CARGO"
rustc_command=("${cargo_command[@]}")
rustc_command[0]="${rustc_command[0]%cargo}rustc"
cd "$ROOT"
taskset -c "$BUILD_CPUS" "${cargo_command[@]}" build --release --locked \
    -p holos-tda --example v010_region_witness

binary="target/release/examples/v010_region_witness"
commit="$(git rev-parse HEAD)"
binary_sha="$(sha256sum "$binary" | awk '{print $1}')"
cargo_version="$("${cargo_command[@]}" --version)"
rustc_version="$("${rustc_command[@]}" -V)"

{
    echo "# Fixed strict-containment witness"
    echo
    echo "- commit: \`$commit\`"
    echo "- binary: \`$binary\`"
    echo "- binary sha256: \`$binary_sha\`"
    echo "- build: \`$CARGO build --release --locked -p holos-tda --example v010_region_witness\`"
    echo "- cargo: $cargo_version"
    echo "- rustc: $rustc_version"
    echo
    echo "The body is deterministic. It contains no timing measurements."
    echo
    echo '```text'
    "$binary"
    echo '```'
} | tee "$OUTPUT"

linux_home='/''home/[^/[:space:]]+'
macos_home='/''Users/[^/[:space:]]+'
windows_home='[A-Za-z]:\\Users\\[^\\[:space:]]+'
if grep -Eq "$linux_home|$macos_home|$windows_home" "$OUTPUT"; then
    echo "error: generated record contains a local user path" >&2
    exit 1
fi
