#!/usr/bin/env bash
# Build and run the registered synthetic bipersistence study.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
CARGO="${CARGO:-cargo +1.92}"
AFFINITY="${AFFINITY:-0-3,12-15}"
BUILD_CPUS="${BUILD_CPUS:-16-31}"

if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" && "${ALLOW_DIRTY:-}" != "1" ]]; then
    echo "error: commit the worktree before a registered study, or set ALLOW_DIRTY=1" >&2
    exit 1
fi
if ! command -v taskset >/dev/null 2>&1; then
    echo "error: taskset is required to pin the registered study" >&2
    exit 1
fi

read -r -a cargo_command <<<"$CARGO"
cd "$ROOT"
taskset -c "$BUILD_CPUS" "${cargo_command[@]}" build --release --locked \
    -p holos-tda -p holos-tda-check -p research-bench

taskset -c "$AFFINITY" env V09_AFFINITY="$AFFINITY" \
    python3 -m benchmarks.v09_bipersistence.run \
    --root "$ROOT" \
    --holos "$ROOT/target/release/holos" \
    --checker "$ROOT/target/release/holos-check" \
    --research-bench "$ROOT/target/release/research-bench" \
    "$@"
