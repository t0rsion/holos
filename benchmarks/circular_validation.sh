#!/usr/bin/env bash
# Run the frozen synthetic and Gardner circular-coordinate studies.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
CARGO="${CARGO:-cargo}"
PYTHON="${CIRCULAR_PYTHON:-python3}"
MODE="${1:---synthetic}"

case "$MODE" in
    --synthetic | --neuroscience | --all) ;;
    -h | --help)
        cat <<'EOF'
Usage: circular_validation.sh [--synthetic|--neuroscience|--all]

Run under taskset -c 0-3,12-15. CIRCULAR_PYTHON must name an environment
with the current holos wheel and circular_requirements.txt installed.
The neuroscience study also needs GARDNER_ARCHIVE and GARDNER_SOURCE.
EOF
        exit 0
        ;;
    *)
        echo "unknown mode: $MODE" >&2
        exit 2
        ;;
esac

if [[ -n "$(git -C "$ROOT" status --porcelain)" && "${ALLOW_DIRTY:-}" != "1" ]]; then
    echo "error: commit the worktree before a registered study" >&2
    exit 1
fi

HEAD="$(git -C "$ROOT" rev-parse --short=12 HEAD)"
WHEEL_HEAD="$($PYTHON -c 'import holos_tda; print(holos_tda.GIT_HASH[:12])')"
if [[ "$HEAD" != "$WHEEL_HEAD" ]]; then
    echo "error: holos wheel is from $WHEEL_HEAD, expected $HEAD" >&2
    exit 1
fi

$PYTHON - "$HERE/circular_requirements.txt" <<'PY'
import importlib.metadata
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line or line.startswith("#"):
        continue
    name, expected = line.split("==", 1)
    actual = importlib.metadata.version(name)
    if actual != expected:
        raise SystemExit(f"{name} is {actual}, expected {expected}")
PY
$CARGO build --release -p holos-tda-check --locked --manifest-path "$ROOT/Cargo.toml"
CHECKER="$ROOT/target/release/holos-check"

if [[ "$MODE" == "--synthetic" || "$MODE" == "--all" ]]; then
    HOLOS_CHECK="$CHECKER" "$PYTHON" "$HERE/circular_validation.py"
fi

if [[ "$MODE" == "--neuroscience" || "$MODE" == "--all" ]]; then
    if [[ -z "${GARDNER_ARCHIVE:-}" || -z "${GARDNER_SOURCE:-}" ]]; then
        echo "error: set GARDNER_ARCHIVE and GARDNER_SOURCE" >&2
        exit 1
    fi
    HOLOS_CHECK="$CHECKER" "$PYTHON" "$HERE/circular_neuroscience.py" \
        --archive "$GARDNER_ARCHIVE" --source "$GARDNER_SOURCE"
fi
