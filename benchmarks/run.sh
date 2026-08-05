#!/usr/bin/env bash
# Benchmark holos against ripser on identical lower-distance inputs.
# Methodology: benchmarks/README.md. Results land in benchmarks/results.txt
# (full log) and benchmarks/results.md (provenance header plus table, ready
# to paste into README verbatim).
#
# Exits nonzero if any run's diagrams disagree. A timing whose diagrams do
# not match is void.
#
# Requires RIPSER_BIN pointing at a ripser binary, such as a local build at
# ../holos/external_ripser/ripser. To build one:
#   git clone https://github.com/Ripser/ripser && make -C ripser
#
# CARGO may carry a toolchain, e.g. CARGO="cargo +1.92" ./run.sh.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
DATA="$HERE/data"
RESULTS="$HERE/results.txt"
RESULTS_MD="$HERE/results.md"
CARGO="${CARGO:-cargo}"
RUSTC="${CARGO/#cargo/rustc}"

SIZES=(200 500 1000 2000)
COORD_DIM=3
SEED=42
MAXDIM=1
FIXED_THRESHOLD=0.5
TOLERANCE=1e-5
# One deeper case on the mid-size cloud.
DIM2_N=500
DIM2_THRESHOLD=0.4

if [[ -z "${RIPSER_BIN:-}" ]]; then
    cat >&2 <<'EOF'
RIPSER_BIN is not set.

Point it at a ripser binary (https://github.com/Ripser/ripser):

    git clone https://github.com/Ripser/ripser
    make -C ripser
    RIPSER_BIN=ripser/ripser ./run.sh
EOF
    exit 1
fi

if [[ ! -r /proc/self/status ]]; then
    echo "measure.py reads peak RSS from /proc (Linux only); no /proc here." >&2
    exit 1
fi

sha256() {
    (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'
}

# measure OUT CMD... : run CMD with stdout redirected to OUT, then print
# "wall_s=<s> max_rss_kb=<kb>". OUT is a real file, never a pipe: a large
# output deadlocks on a full pipe buffer. See measure.py.
measure() {
    local out="$1"
    shift
    python3 "$HERE/measure.py" "$out" "$@"
}

kb_to_mb() {
    awk -v kb="$1" 'BEGIN { printf "%.1f", kb / 1024 }'
}

# Point cloud CSV -> ripser lower-distance format (condensed lower triangle,
# row by row), so both tools read the exact same distance file.
cloud_to_lower() {
    python3 - "$1" <<'EOF'
import math
import sys

pts = []
with open(sys.argv[1]) as f:
    for line in f:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        pts.append([float(t) for t in line.replace(",", " ").split()])
for i in range(1, len(pts)):
    print(" ".join(repr(math.dist(pts[i], pts[j])) for j in range(i)))
EOF
}

# Compare two ripser-format outputs as interval multisets per dimension, with
# endpoints within TOLERANCE. Matching is greedy over sorted bars rather than
# a positional zip: ripser prints f32-rounded values, so near-equal births can
# sort in a different order than holos's f64 output. A positional comparison
# misaligns from there on. Prints yes/no.
compare_diagrams() {
    python3 - "$1" "$2" "$TOLERANCE" <<'EOF'
import re
import sys

HEADER = re.compile(r"persistence intervals in dim (\d+):")
INTERVAL = re.compile(r"\s*\[([^,\[\]]*),([^)\[\]]*)\)\s*$")


def parse(path):
    bars = {}
    dim = None
    with open(path) as f:
        for line in f:
            m = HEADER.match(line)
            if m:
                dim = int(m.group(1))
                bars.setdefault(dim, [])
                continue
            m = INTERVAL.match(line)
            if m and dim is not None:
                death = m.group(2).strip()
                bars[dim].append(
                    (float(m.group(1)), float("inf") if death == "" else float(death))
                )
    return bars


a, b = parse(sys.argv[1]), parse(sys.argv[2])
tol = float(sys.argv[3])


def close(x, y):
    return x == y or abs(x - y) <= tol


def bars_match(p, q):
    return close(p[0], q[0]) and close(p[1], q[1])


ok = True
for dim in sorted(set(a) | set(b)):
    u, v = sorted(a.get(dim, [])), sorted(b.get(dim, []))
    if len(u) != len(v):
        ok = False
        break
    unmatched = list(v)
    for p in u:
        for i, q in enumerate(unmatched):
            if bars_match(p, q):
                del unmatched[i]
                break
        else:
            ok = False
            break
    if not ok or unmatched:
        ok = False
        break
print("yes" if ok else "no")
EOF
}

mkdir -p "$DATA"

BUILD_CMD="$CARGO build --release -p holos-tda --manifest-path $ROOT/Cargo.toml"
$BUILD_CMD
HOLOS_BIN="$ROOT/target/release/holos"
# Recorded output shows repo-relative and basename paths only. An absolute
# path carries no provenance value and leaks the local machine layout.
BUILD_CMD_DISPLAY="$CARGO build --release -p holos-tda"
HOLOS_BIN_DISPLAY="target/release/holos"
RIPSER_BIN_DISPLAY="$(basename "$RIPSER_BIN")"

# [profile.release] flags from Cargo.toml, verbatim.
PROFILE_FLAGS="$(sed -n '/^\[profile\.release\]/,/^\[/{/^\[profile\.release\]/d;/^\[/d;/^[[:space:]]*$/d;p;}' "$ROOT/Cargo.toml" | tr '\n' ';' | sed 's/;$//;s/;/; /g')"

# Ripser compile flags, if a Makefile sits next to the binary (vendored
# build). Otherwise they are unknowable from here.
RIPSER_DIR="$(cd "$(dirname "$RIPSER_BIN")" && pwd)"
if [[ -f "$RIPSER_DIR/Makefile" ]]; then
    RIPSER_FLAGS="$(awk '/^ripser:/ { getline; sub(/^\t+/, ""); print; exit }' "$RIPSER_DIR/Makefile") [from the Makefile beside the binary]"
else
    RIPSER_FLAGS="unknown (no Makefile beside the binary)"
fi

DATE_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
HOLOS_COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
# A dirty worktree means the binary may not correspond to the recorded
# commit. That failure mode produced the predecessor's phantom benchmark
# win. Refuse unless explicitly overridden (recorded as DIRTY).
DIRTY="$(git -C "$ROOT" status --porcelain 2>/dev/null)"
if [[ -n "$DIRTY" ]]; then
    if [[ "${ALLOW_DIRTY:-}" != "1" ]]; then
        echo "error: worktree is dirty; commit first or set ALLOW_DIRTY=1" >&2
        git -C "$ROOT" status --porcelain >&2
        exit 1
    fi
    HOLOS_COMMIT="$HOLOS_COMMIT-DIRTY"
fi
RUSTFLAGS_RECORD="${RUSTFLAGS:-<unset>}"
HOLOS_SHA="$(sha256 "$HOLOS_BIN")"
RIPSER_SHA="$(sha256 "$RIPSER_BIN")"
HOLOS_VERSION="$("$HOLOS_BIN" --version)"
CARGO_VERSION="$($CARGO --version)"
RUSTC_VERSION="$($RUSTC -V)"
CC_VERSION="$( (cc --version 2>/dev/null || echo unknown) | head -1)"
CPU="$( (grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//') 2>/dev/null || sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
AFFINITY="$( (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown)"

{
    echo "holos benchmark run"
    echo "date: $DATE_UTC"
    echo "holos commit: $HOLOS_COMMIT"
    echo "holos binary: $HOLOS_BIN_DISPLAY"
    echo "holos sha256: $HOLOS_SHA"
    echo "holos version: $HOLOS_VERSION"
    echo "build command: $BUILD_CMD_DISPLAY"
    echo "[profile.release]: $PROFILE_FLAGS"
    echo "RUSTFLAGS: $RUSTFLAGS_RECORD"
    echo "ripser binary: $RIPSER_BIN_DISPLAY"
    echo "ripser sha256: $RIPSER_SHA"
    echo "ripser build: $RIPSER_FLAGS"
    echo "cargo: $CARGO_VERSION"
    echo "rustc: $RUSTC_VERSION"
    # The cc version is a proxy only. How RIPSER_BIN was built is not
    # knowable from here unless it was built on this machine.
    echo "cc: $CC_VERSION"
    echo "cpu: $CPU"
    echo "cpus allowed: $AFFINITY"
    echo "threads: 1 (both binaries are serial)"
    echo "timing: benchmarks/measure.py (monotonic wall clock; peak RSS = VmHWM sampled at 0.5-10 ms, exec-gated)"
    echo "sizes: ${SIZES[*]}  coord dim: $COORD_DIM  seed: $SEED  maxdim: $MAXDIM"
    echo "fixed threshold: $FIXED_THRESHOLD  dim-2 case: n=$DIM2_N threshold=$DIM2_THRESHOLD"
    echo "agreement tolerance: $TOLERANCE"
    echo
} >"$RESULTS"

{
    echo "<!-- Generated by benchmarks/run.sh. Do not edit; rerun the script. -->"
    echo
    echo "- date: $DATE_UTC"
    echo "- holos commit: $HOLOS_COMMIT"
    echo "- holos binary: \`$HOLOS_BIN_DISPLAY\` sha256 \`$HOLOS_SHA\`"
    echo "- holos version: $HOLOS_VERSION"
    echo "- build: \`$BUILD_CMD_DISPLAY\` with \`[profile.release]\` $PROFILE_FLAGS; RUSTFLAGS \`$RUSTFLAGS_RECORD\`"
    echo "- rustc: $RUSTC_VERSION ($CARGO_VERSION)"
    echo "- ripser binary: \`$RIPSER_BIN_DISPLAY\` sha256 \`$RIPSER_SHA\`"
    echo "- ripser build: \`$RIPSER_FLAGS\`"
    echo "- cpu: $CPU; cpus allowed: $AFFINITY; threads: 1 (both binaries are serial)"
    echo "- input: uniform unit-cube clouds, coord dim $COORD_DIM, seed $SEED (gen_cloud.py); identical lower-distance file fed to both tools"
    echo "- timing: measure.py (monotonic wall clock; peak RSS = VmHWM sampled at 0.5-10 ms, exec-gated); agreement tolerance $TOLERANCE"
    echo
    echo "| N | threshold | maxdim | holos time (s) | ripser time (s) | holos peak RSS (MB) | ripser peak RSS (MB) | diagrams match |"
    echo "|--:|:--|--:|--:|--:|--:|--:|:--|"
} >"$RESULTS_MD"

ANY_MISMATCH=0

# run_case N LABEL THRESHOLD_DESC MAXDIM [THRESHOLD]
# THRESHOLD absent = no --threshold on either side. Both tools then fall back
# to the enclosing radius, the fair full-persistence run.
run_case() {
    local n="$1" label="$2" tdesc="$3" maxdim="$4" threshold="${5:-}"
    local lower="$DATA/cloud_${n}.lower"
    local holos_cmd ripser_cmd

    holos_cmd=("$HOLOS_BIN" "$lower" --format lower-distance --dim "$maxdim")
    ripser_cmd=("$RIPSER_BIN" --format lower-distance --dim "$maxdim" "$lower")
    if [[ -n "$threshold" ]]; then
        holos_cmd+=(--threshold "$threshold")
        ripser_cmd+=(--threshold "$threshold")
    fi

    local holos_out="$DATA/holos_${n}_${label}.out"
    local ripser_out="$DATA/ripser_${n}_${label}.out"
    local holos_stats ripser_stats
    holos_stats="$(measure "$holos_out" "${holos_cmd[@]}")"
    ripser_stats="$(measure "$ripser_out" "${ripser_cmd[@]}")"

    local holos_wall holos_rss ripser_wall ripser_rss
    holos_wall="${holos_stats#wall_s=}"
    holos_wall="${holos_wall%% *}"
    holos_rss="${holos_stats##*max_rss_kb=}"
    ripser_wall="${ripser_stats#wall_s=}"
    ripser_wall="${ripser_wall%% *}"
    ripser_rss="${ripser_stats##*max_rss_kb=}"

    local match
    match="$(compare_diagrams "$holos_out" "$ripser_out")"
    if [[ "$match" != yes ]]; then
        ANY_MISMATCH=1
    fi

    {
        echo "== n=$n case=$label maxdim=$maxdim"
        echo "holos cmd:  ${holos_cmd[*]}"
        echo "holos   $holos_stats"
        echo "ripser cmd: ${ripser_cmd[*]}"
        echo "ripser  $ripser_stats"
        echo "DIAGRAMS_MATCH $match"
        echo
    } >>"$RESULTS"

    echo "| $n | $tdesc | $maxdim | $holos_wall | $ripser_wall | $(kb_to_mb "$holos_rss") | $(kb_to_mb "$ripser_rss") | $match |" >>"$RESULTS_MD"
    echo "n=$n case=$label maxdim=$maxdim DIAGRAMS_MATCH=$match" >&2
}

for n in "${SIZES[@]}"; do
    cloud="$DATA/cloud_${n}.csv"
    lower="$DATA/cloud_${n}.lower"
    python3 "$HERE/gen_cloud.py" "$n" "$COORD_DIM" "$SEED" >"$cloud"
    cloud_to_lower "$cloud" >"$lower"

    run_case "$n" default "enclosing radius" "$MAXDIM"
    run_case "$n" fixed "$FIXED_THRESHOLD" "$MAXDIM" "$FIXED_THRESHOLD"
done

# Deeper case: maxdim 2 on the mid-size cloud (its .lower already exists).
run_case "$DIM2_N" dim2 "$DIM2_THRESHOLD" 2 "$DIM2_THRESHOLD"

echo "Results written to $RESULTS and $RESULTS_MD." >&2
echo "Do not copy numbers into documents by hand; rerun this script instead." >&2

if [[ "$ANY_MISMATCH" -ne 0 ]]; then
    echo "FAILURE: at least one run's diagrams disagree; timings above are void." >&2
    exit 1
fi
