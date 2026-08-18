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

# measure_err OUT ERR CMD... : measure, keeping the child's stderr in ERR.
# Collapse statistics arrive that way.
measure_err() {
    local out="$1" err="$2"
    shift 2
    MEASURE_STDERR="$err" python3 "$HERE/measure.py" "$out" "$@"
}

# median_iqr : read one sample per line on stdin, print
# "median=<x> iqr=<x> q1=<x> q3=<x> min=<x> max=<x>". Quantiles interpolate
# linearly between the two neighbouring order statistics, the rule numpy uses
# by default.
median_iqr() {
    sort -g | awk '
        { v[NR] = $1 }
        function q(p,   h, lo, fr) {
            h = (NR - 1) * p
            lo = int(h)
            fr = h - lo
            if (lo + 2 > NR) return v[NR]
            return v[lo + 1] + fr * (v[lo + 2] - v[lo + 1])
        }
        END {
            if (NR == 0) {
                print "median=0 iqr=0 q1=0 q3=0 min=0 max=0"
                exit
            }
            printf "median=%.4f iqr=%.4f q1=%.4f q3=%.4f min=%.4f max=%.4f\n",
                q(0.5), q(0.75) - q(0.25), q(0.25), q(0.75), v[1], v[NR]
        }
    '
}

# measure_repeat OUT ERR REPS CMD... : one warm-up run, then REPS timed runs.
# Prints "median_s=<s> iqr_s=<s> q1_s=<s> q3_s=<s> min_s=<s> max_s=<s>
# max_rss_kb=<kb> runs=<n>". Peak RSS is the largest of the timed runs. OUT
# and ERR hold the last run's output. A run that exits nonzero returns 1 and
# prints nothing, so a caller that guards the call sees the failure even
# where errexit is suspended.
measure_repeat() {
    local out="$1" err="$2" reps="$3"
    shift 3
    measure_err "$out" "$err" "$@" >/dev/null || return 1
    local walls=() rss_peak=0 line wall rss r
    for ((r = 0; r < reps; r++)); do
        line="$(measure_err "$out" "$err" "$@")" || return 1
        wall="${line#wall_s=}"
        wall="${wall%% *}"
        rss="${line##*max_rss_kb=}"
        walls+=("$wall")
        if ((rss > rss_peak)); then
            rss_peak="$rss"
        fi
    done
    local stats med iqr q1 q3 lo hi
    stats="$(printf '%s\n' "${walls[@]}" | median_iqr)"
    med="${stats#median=}"
    med="${med%% *}"
    iqr="${stats#* iqr=}"
    iqr="${iqr%% *}"
    q1="${stats#* q1=}"
    q1="${q1%% *}"
    q3="${stats#* q3=}"
    q3="${q3%% *}"
    lo="${stats#* min=}"
    lo="${lo%% *}"
    hi="${stats##* max=}"
    echo "median_s=$med iqr_s=$iqr q1_s=$q1 q3_s=$q3 min_s=$lo max_s=$hi max_rss_kb=$rss_peak runs=$reps"
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
    $CARGO build --release -p holos-tda --manifest-path "$ROOT/Cargo.toml" >&2
    HOLOS_BIN="$ROOT/target/release/holos"
    HOLOS_BIN_DISPLAY="target/release/holos"
    BUILD_CMD_DISPLAY="$CARGO build --release -p holos-tda"
}

# emit_provenance FILE HEADER : write the provenance block that ties every
# timing below it to an exact build. Refuses a dirty worktree unless
# ALLOW_DIRTY=1 (recorded as -DIRTY), as run.sh does. The fields stay
# readable afterwards in PROV_*, so a second results file can carry the same
# header without probing the machine twice.
emit_provenance() {
    local file="$1" header="$2"

    PROV_PROFILE_FLAGS="$(sed -n '/^\[profile\.release\]/,/^\[/{/^\[profile\.release\]/d;/^\[/d;/^[[:space:]]*$/d;p;}' "$ROOT/Cargo.toml" | tr '\n' ';' | sed 's/;$//;s/;/; /g')"

    local dirty
    PROV_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    PROV_COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
    dirty="$(git -C "$ROOT" status --porcelain 2>/dev/null)"
    if [[ -n "$dirty" ]]; then
        if [[ "${ALLOW_DIRTY:-}" != "1" ]]; then
            echo "error: worktree is dirty; commit first or set ALLOW_DIRTY=1" >&2
            git -C "$ROOT" status --porcelain >&2
            exit 1
        fi
        PROV_COMMIT="$PROV_COMMIT-DIRTY"
    fi

    PROV_CPU="$( (grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//') 2>/dev/null || sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
    PROV_NCPU="$( (nproc) 2>/dev/null || echo unknown)"
    PROV_AFFINITY="$( (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown)"
    # Physical topology of the allowed CPUs: SMT siblings share a core,
    # so "8 logical" can be 4 physical cores. Interpret scaling per core.
    PROV_TOPOLOGY="$(lscpu -p=CPU,CORE,SOCKET 2>/dev/null | awk -F, -v allowed="$PROV_AFFINITY" '
        BEGIN {
            n = split(allowed, parts, ",")
            for (i = 1; i <= n; i++) {
                if (split(parts[i], r, "-") == 2) { for (c = r[1]; c <= r[2]; c++) ok[c] = 1 }
                else { ok[parts[i]] = 1 }
            }
        }
        # logical:socket.core. Core ids repeat across sockets, so the
        # socket belongs to the identity of a core.
        /^[0-9]/ { if ($1 in ok) { printf "%s%s:%s.%s", sep, $1, $3, $2; sep = " " } }
    ' || echo unknown)"
    [ -n "$PROV_TOPOLOGY" ] || PROV_TOPOLOGY=unknown
    PROV_HOLOS_SHA="$(sha256 "$HOLOS_BIN")"
    PROV_HOLOS_VERSION="$("$HOLOS_BIN" --version)"
    PROV_CARGO_VERSION="$($CARGO --version)"
    PROV_RUSTC_VERSION="$($RUSTC -V)"

    {
        echo "$header"
        echo "date: $PROV_DATE"
        echo "holos commit: $PROV_COMMIT"
        echo "holos binary: $HOLOS_BIN_DISPLAY"
        echo "holos sha256: $PROV_HOLOS_SHA"
        echo "holos version: $PROV_HOLOS_VERSION"
        echo "build command: $BUILD_CMD_DISPLAY"
        echo "[profile.release]: $PROV_PROFILE_FLAGS"
        echo "RUSTFLAGS: ${RUSTFLAGS:-<unset>}"
        echo "cargo: $PROV_CARGO_VERSION"
        echo "rustc: $PROV_RUSTC_VERSION"
        echo "cpu: $PROV_CPU ($PROV_NCPU logical; cpus allowed: $PROV_AFFINITY)"
        echo "cpu topology of allowed set (logical:socket.core): $PROV_TOPOLOGY"
        echo "timing: benchmarks/measure.py (monotonic wall clock; peak RSS = VmHWM, exec-gated)"
        echo
    } >"$file"
}

# emit_provenance_md : the same identity as the text header, as markdown
# bullets on stdout. emit_provenance must run first.
emit_provenance_md() {
    echo "- date: $PROV_DATE"
    echo "- holos commit: $PROV_COMMIT"
    echo "- holos binary: \`$HOLOS_BIN_DISPLAY\` sha256 \`$PROV_HOLOS_SHA\`"
    echo "- holos version: $PROV_HOLOS_VERSION"
    echo "- build: \`$BUILD_CMD_DISPLAY\` with \`[profile.release]\` $PROV_PROFILE_FLAGS; RUSTFLAGS \`${RUSTFLAGS:-<unset>}\`"
    echo "- rustc: $PROV_RUSTC_VERSION ($PROV_CARGO_VERSION)"
    echo "- cpu: $PROV_CPU ($PROV_NCPU logical); cpus allowed: $PROV_AFFINITY"
    echo "- cpu topology of allowed set (logical:socket.core): $PROV_TOPOLOGY"
    echo "- timing: measure.py (monotonic wall clock; peak RSS = VmHWM, exec-gated)"
}

# field KEY LINE : print the value of KEY in a "k=v k=v" line, empty if absent.
field() {
    awk -v key="$1" '{
        for (i = 1; i <= NF; i++) {
            if (index($i, key "=") == 1) {
                print substr($i, length(key) + 2)
                exit
            }
        }
    }' <<<"$2"
}

require_proc() {
    if [[ ! -r /proc/self/status ]]; then
        echo "measure.py reads peak RSS from /proc (Linux only); no /proc here." >&2
        exit 1
    fi
}

# head_commit : the commit this checkout sits on, with the -DIRTY suffix
# emit_provenance appends, or "unknown". The manifest records the same string.
head_commit() {
    local commit
    commit="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
    if [[ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ]]; then
        commit="$commit-DIRTY"
    fi
    echo "$commit"
}
