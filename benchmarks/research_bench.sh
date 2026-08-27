#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
output="${OUTPUT:-$repo/benchmarks/results_research.md}"
repetitions="${REPS:-5}"

cd "$repo"
cargo +1.92 build --release --locked -p research-bench
{
    echo "# Certified workflow study"
    echo
    echo '```text'
    taskset -c 0-3,12-15 target/release/research-bench --reps "$repetitions"
    echo '```'
} | tee "$output"
