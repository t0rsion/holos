#!/usr/bin/env bash
# Run the external degree-Rips validation harness.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
PYTHON="${MULTIPERS_PYTHON:-python3.12}"
AFFINITY="${AFFINITY:-0-3,12-15}"

if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" && "${ALLOW_DIRTY:-}" != "1" ]]; then
    echo "error: commit the worktree before a registered run, or set ALLOW_DIRTY=1 for a local diagnostic" >&2
    exit 1
fi
if ! command -v taskset >/dev/null 2>&1; then
    echo "error: taskset is required to pin the external validation" >&2
    exit 1
fi

if ! command -v "$PYTHON" >/dev/null 2>&1 && [[ ! -x "$PYTHON" ]]; then
    echo "error: set MULTIPERS_PYTHON to a Python 3.12 interpreter with the pinned external packages" >&2
    exit 1
fi

exec taskset -c "$AFFINITY" python3 "$HERE/external_bipersistence.py" \
    --root "$ROOT" \
    --input "$HERE/external_bipersistence_cases.json" \
    --holos "${HOLOS_BIN:-$ROOT/target/release/holos}" \
    --multipers-python "$PYTHON" \
    --affinity "$AFFINITY" \
    "$@"
