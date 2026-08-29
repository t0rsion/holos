#!/usr/bin/env bash
set -euo pipefail

CARGO="${CARGO:-cargo +1.92}"
# shellcheck source=benchmarks/_common.sh
source "$(dirname "$0")/_common.sh"
read -r -a cargo_command <<<"$CARGO"
rustc_command=("${cargo_command[@]}")
rustc_command[0]="${rustc_command[0]%cargo}rustc"

output="${OUTPUT:-$ROOT/benchmarks/results_research.md}"
repetitions="${REPS:-5}"
affinity="0-3,12-15"
build_affinity="${BUILD_CPUS:-16-31}"
binary="$ROOT/target/release/research-bench"

cd "$ROOT"
if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: commit the worktree before recording the study" >&2
    git status --porcelain >&2
    exit 1
fi

taskset -c "$build_affinity" "${cargo_command[@]}" \
    build --release --locked -p research-bench
commit="$(git rev-parse HEAD)"
binary_sha="$(sha256 "$binary")"
recorded_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cpu="$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
cargo_version="$("${cargo_command[@]}" --version)"
rustc_version="$("${rustc_command[@]}" -V)"

{
    echo "# Certified workflow study"
    echo
    echo "- recorded: $recorded_at"
    echo "- commit: \`$commit\`"
    echo "- binary: \`target/release/research-bench\`"
    echo "- binary sha256: \`$binary_sha\`"
    echo "- build: \`$CARGO build --release --locked -p research-bench\`"
    echo "- cargo: $cargo_version"
    echo "- rustc: $rustc_version"
    echo "- cpu: $cpu"
    echo "- affinity: \`$affinity\`"
    echo "- repetitions: $repetitions"
    echo
    echo "The cases are fixed integration gates. Timings are descriptive."
    echo
    echo '```text'
    taskset -c "$affinity" "$binary" --reps "$repetitions"
    echo '```'
} | tee "$output"

linux_home='/''home/[^/[:space:]]+'
macos_home='/''Users/[^/[:space:]]+'
windows_home='[A-Za-z]:\\Users\\[^\\[:space:]]+'
if grep -Eq "$linux_home|$macos_home|$windows_home" "$output"; then
    echo "error: generated record contains a local user path" >&2
    exit 1
fi
