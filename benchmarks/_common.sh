# Shared helpers for the parallel and sparse benchmark scripts. Sourced, not
# run.
#
# Provenance discipline, inherited from run.sh: recorded output carries the
# holos commit, binary sha256, build flags, and CPU. It shows repo-relative or
# basename paths ONLY. It must never carry an absolute path, a home directory,
# or a username.

# Resolve HERE/ROOT/DATA relative to the sourcing script's own location.
HERE="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
DATA="$HERE/data"
CARGO="${CARGO:-cargo}"
RUSTC="${CARGO/#cargo/rustc}"

sha256() {
    (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'
}

kb_to_mb() {
    awk -v kb="$1" 'BEGIN { printf "%.1f", kb / 1024 }'
}

# measure OUT CMD... : run CMD with stdout redirected to OUT, then print
# "wall_s=<s> max_rss_kb=<kb>". OUT is a real file, never a pipe. See
# measure.py.
measure() {
    local out="$1"
    shift
    python3 "$HERE/measure.py" "$out" "$@"
}

# speedup BASE_WALL WALL -> BASE_WALL / WALL, "n/a" if either is zero.
speedup() {
    awk -v b="$1" -v w="$2" 'BEGIN { if (w+0 == 0 || b+0 == 0) print "n/a"; else printf "%.2f", b / w }'
}

# Point cloud CSV -> condensed lower-distance (ripser/holos lower-distance
# format), so a cloud and its distance matrix carry identical geometry.
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
# endpoints within TOLERANCE (default 1e-5). Matching is greedy over sorted
# bars, as in run.sh: f32-rounded endpoints can sort differently than holos's
# f64 output. Prints yes/no.
compare_diagrams() {
    python3 - "$1" "$2" "${TOLERANCE:-1e-5}" <<'EOF'
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


ok = True
for dim in sorted(set(a) | set(b)):
    u, v = sorted(a.get(dim, [])), sorted(b.get(dim, []))
    if len(u) != len(v):
        ok = False
        break
    unmatched = list(v)
    for p in u:
        for i, q in enumerate(unmatched):
            if close(p[0], q[0]) and close(p[1], q[1]):
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

# Build the release holos binary. Sets HOLOS_BIN plus display strings that
# carry no absolute path.
build_holos() {
    $CARGO build --release --manifest-path "$ROOT/Cargo.toml" >&2
    HOLOS_BIN="$ROOT/target/release/holos"
    HOLOS_BIN_DISPLAY="target/release/holos"
    BUILD_CMD_DISPLAY="$CARGO build --release"
}

# emit_provenance FILE HEADER : write the provenance block that ties every
# timing below it to an exact build. Refuses a dirty worktree unless
# ALLOW_DIRTY=1 (recorded as -DIRTY), as run.sh does.
emit_provenance() {
    local file="$1" header="$2"

    local profile_flags
    profile_flags="$(sed -n '/^\[profile\.release\]/,/^\[/{/^\[profile\.release\]/d;/^\[/d;/^[[:space:]]*$/d;p;}' "$ROOT/Cargo.toml" | tr '\n' ';' | sed 's/;$//;s/;/; /g')"

    local date_utc commit dirty
    date_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    commit="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
    dirty="$(git -C "$ROOT" status --porcelain 2>/dev/null)"
    if [[ -n "$dirty" ]]; then
        if [[ "${ALLOW_DIRTY:-}" != "1" ]]; then
            echo "error: worktree is dirty; commit first or set ALLOW_DIRTY=1" >&2
            git -C "$ROOT" status --porcelain >&2
            exit 1
        fi
        commit="$commit-DIRTY"
    fi

    local cpu ncpu affinity
    cpu="$( (grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//') 2>/dev/null || sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
    ncpu="$( (nproc) 2>/dev/null || echo unknown)"
    affinity="$( (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown)"

    {
        echo "$header"
        echo "date: $date_utc"
        echo "holos commit: $commit"
        echo "holos binary: $HOLOS_BIN_DISPLAY"
        echo "holos sha256: $(sha256 "$HOLOS_BIN")"
        echo "holos version: $("$HOLOS_BIN" --version)"
        echo "build command: $BUILD_CMD_DISPLAY"
        echo "[profile.release]: $profile_flags"
        echo "RUSTFLAGS: ${RUSTFLAGS:-<unset>}"
        echo "cargo: $($CARGO --version)"
        echo "rustc: $($RUSTC -V)"
        echo "cpu: $cpu ($ncpu logical; cpus allowed: $affinity)"
        echo "timing: benchmarks/measure.py (monotonic wall clock; peak RSS = VmHWM, exec-gated)"
        echo
    } >"$file"
}

require_proc() {
    if [[ ! -r /proc/self/status ]]; then
        echo "measure.py reads peak RSS from /proc (Linux only); no /proc here." >&2
        exit 1
    fi
}
