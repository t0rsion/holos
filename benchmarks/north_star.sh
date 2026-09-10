#!/usr/bin/env bash
# Registered north-star engine study. The corpus is
# benchmarks/north_star_corpus.toml, and its [meta] block quotes
# the decision rule, the noise rule, the pinning rule, the arms, and the
# timing protocol. Neither the corpus nor this runner changes in response
# to a result.
#
# This study produces the public performance statements of the release. The
# engineering harness (engine_bench.sh) does not; it tunes the engine and
# decides landing on disclosed data.
#
# One frozen pass, three parts:
#   serial     every holos arm, the A/A control, and one ripser build on
#              one physical core. The serial grade reads it
#   multicore  H3 and giotto-ph 0.2.4, both at four physical cores, on the
#              entries that carry multicore = true. The multicore grade
#              reads it
#   scaling    the [[scaling]] entries: H3 and giotto-ph at 1, 2, and 4
#              physical cores, with H2 at four cores as the attribution
#              diagnostic. Descriptive, no grade
#
# Timing. holos and ripser are fresh processes, timed by
# benchmarks/measure.py: one warm-up run, then the timed repetitions. The
# warm-up run is also the agreement run. giotto-ph has no command-line
# tool, so it is timed in process the way benchmarks/giotto_compare.sh
# times it: one monotonic clock around the ripser_parallel call alone. That
# clock excludes process start and input parse, which holos pays inside its
# number, so the comparison favors giotto-ph. The record says so.
#
# Rotation. One repetition runs every command of the entry once, and
# repetition r starts the cycle one place further on. The repetition count
# is the smallest multiple of the command count at or above REPS, so every
# command takes every position equally often. An unbalanced entry is void.
#
# A/A control. The runner copies the H3 binary, checks that the copy hashes
# the same, and times it as an arm. Its per-entry ratios define the noise
# band, which benchmarks/north_star_tables.py computes as the 95th
# percentile of their absolute distance from 1.0. No band is typed in.
#
# Agreement. Every arm and every competitor is compared against the H3
# diagram as interval multisets within TOLERANCE. A mismatch voids the
# entry.
#
# Controller. Amendment 1 of the corpus keeps the harness off the timed
# cores: the runner re-execs itself on one logical CPU outside every timed
# physical core, so its own process, measure.py, and the peak RSS sampler
# never share a core with a run they time. The timed children get their own
# affinity from MEASURE_AFFINITY, and taskset pins giotto-ph, so no child is
# held to the controller CPU. Untimed work, the arm builds and the input
# generation, runs on the allowed CPU set.
#
# Freeze. The corpus is hashed before the first input is generated and
# again before the first run is timed. A change between the two stops the
# run. The manifest records the hash the run used, every arm's commit and
# binary sha256, every competitor's hash or version, the CPU list, the
# kernel, and the date.
#
# Validity. A complete study is a clean, unfiltered run of the frozen corpus
# with the frozen arms, every competitor the corpus requires, the registered
# giotto-ph, a verified topology, and no void entry. The manifest writes
# study_valid=yes only then, and lists the reasons when it writes no.
#
# Results: benchmarks/results_north_star.txt (log), .md (tables), and
# results_north_star_manifest.txt. All three are gitignored.
#
# CARGO may carry a toolchain: CARGO="cargo +1.92" ./north_star.sh
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: north_star.sh [-h]

Run the registered north-star study over benchmarks/north_star_corpus.toml.
It is one frozen pass: there is no screen and no confirmation set.

Environment:
  RIPSER_BIN      stock f32 ripser binary (required)
  RIPSER_COEFF_BIN
                  ripser built with -D USE_COEFFICIENTS, for the odd-prime
                  entries. Without it those entries run their holos arms
                  and carry no ratio
  RIPSER_F64_BIN  the audited matched-precision build. Optional. It is a
                  diagnostic arm, reported in a table of its own and never
                  pooled with the stock arm
  RIPSER_COEFF_F64_BIN
                  the matched-precision odd-prime build. Optional, same
                  standing
  GPH_PYTHON      python interpreter that imports gph, default python3
  CARGO           cargo invocation (may carry a toolchain), default "cargo"
  CORPUS          corpus file, default benchmarks/north_star_corpus.toml
  HOLOS_ARMS      space-separated label=commit list. The default comes from
                  the corpus [meta.arm_commits] block, and one label must
                  be h3. "@" means the working tree
  HIST_DIR        where the arm checkouts and their target directories
                  live, default benchmarks/data/hist. One arm holds one
                  full build, so point it at a filesystem with room
  REBUILD_HIST    set to 1 to rebuild the arm checkouts even when their
                  binaries exist
  REPS            minimum timed repetitions per command, default 5. Each
                  entry runs the smallest multiple of its command count at
                  or above it, so the rotation stays balanced
  NS_ONLY         comma-separated globs over entry and scaling ids, default
                  all. A filtered run is recorded as filtered and grades
                  nothing
  NS_CONTROLLER_CPU
                  the logical CPU the runner, measure.py, and the peak RSS
                  sampler run on, default [meta.pinning].controller_cpu of
                  the corpus. It must lie outside every timed physical core,
                  and the runner refuses to start when it does not
  TOLERANCE       absolute agreement tolerance, default 1e-5
  ALLOW_DIRTY     set to 1 to run a dirty worktree (recorded as -DIRTY)
  ALLOW_DRAFT     set to 1 to run a version 0 corpus; the run is then void
  ALLOW_ANY_TOPOLOGY
                  set to 1 to run with an unknown topology or a pinning
                  list the allowed CPU set does not satisfy; the run is
                  then void
  ALLOW_NO_GPH    set to 1 to run without giotto-ph; the multicore grade
                  and the CPU scaling table are then not run, and the
                  record says so
  ALLOW_ANY_GPH_VERSION
                  set to 1 to accept another giotto-ph version; the
                  multicore grade is then void
  ALLOW_MISSING_COMPETITORS
                  set to 1 to run without a competitor build the corpus
                  requires; the study is then not complete
EOF
}

for arg in "$@"; do
    case "$arg" in
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $arg" >&2
            usage >&2
            exit 2
            ;;
    esac
done

source "$(dirname "$0")/_common.sh"
require_proc

# CARGO may carry a toolchain ("cargo +1.92"), so it becomes an array before
# on_allowed passes it on.
read -r -a CARGO_CMD <<<"$CARGO"

CORPUS="${CORPUS:-$HERE/north_star_corpus.toml}"
HIST_DIR="${HIST_DIR:-$DATA/hist}"
REPS="${REPS:-5}"
NS_ONLY="${NS_ONLY:-*}"
TOLERANCE="${TOLERANCE:-1e-5}"
GPH_PYTHON="${GPH_PYTHON:-python3}"
export TOLERANCE

if [[ -z "${RIPSER_BIN:-}" ]]; then
    cat >&2 <<'EOF'
RIPSER_BIN is not set.

Point it at a stock ripser binary (https://github.com/Ripser/ripser):

    git clone https://github.com/Ripser/ripser
    make -C ripser
    RIPSER_BIN=ripser/ripser ./north_star.sh
EOF
    exit 1
fi
for name in RIPSER_BIN RIPSER_COEFF_BIN RIPSER_F64_BIN RIPSER_COEFF_F64_BIN; do
    path="${!name:-}"
    if [[ -n "$path" && ! -x "$path" ]]; then
        echo "error: $name=$path is not an executable file" >&2
        exit 1
    fi
done
if ! [[ "$REPS" =~ ^[0-9]+$ ]] || ((REPS < 1)); then
    echo "error: REPS=$REPS is not a positive integer" >&2
    exit 1
fi
if [[ ! -r "$CORPUS" ]]; then
    echo "error: no corpus at $(basename "$CORPUS")" >&2
    exit 1
fi
if ! python3 -c 'import tomllib' 2>/dev/null; then
    echo "error: reading the corpus needs Python 3.11 or newer (tomllib)" >&2
    exit 1
fi

RESULTS="$HERE/results_north_star.txt"
RESULTS_MD="$HERE/results_north_star.md"
MANIFEST="$HERE/results_north_star_manifest.txt"

# corpus_settings : the [meta] fields the runner enforces, as one
# "key=value" line per field. It fails on a missing field, so a corpus that
# lost a rule cannot run.
corpus_settings() {
    python3 - "$CORPUS" <<'EOF'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    corpus = tomllib.load(f)
meta = corpus.get("meta", {})
version, date = meta.get("version"), meta.get("date")
if not isinstance(version, int) or version < 0 or not date:
    sys.exit("corpus [meta] needs an integer version and a date")
pin = meta.get("pinning", {})
arms = meta.get("arm_commits", {})
noise = meta.get("noise", {})
scaling = meta.get("scaling", {})
comp = meta.get("competitor_versions", {})
required = meta.get("required_competitors", {})
for name, table, keys in (
    ("pinning", pin,
     ("physical_cores", "cores_1", "cores_2", "cores_4", "controller_cpu")),
    ("arm_commits", arms, ("h0", "h1", "h2", "h3")),
    ("noise", noise, ("short_run_s", "entry_slack", "band_percentile")),
    ("scaling", scaling, ("threads", "attribution_arm", "attribution_threads")),
    ("competitor_versions", comp, ("gph_version",)),
    ("required_competitors", required,
     ("ripser", "ripser_coeff", "ripser_f64")),
):
    missing = [k for k in keys if k not in table]
    if missing:
        sys.exit(f"corpus [meta.{name}] lacks: {', '.join(missing)}")
print(f"version={version}")
print(f"date={date}")
print(f"physical_cores={pin['physical_cores']}")
for k in ("cores_1", "cores_2", "cores_4"):
    print(f"{k}={pin[k]}")
print("arms=" + " ".join(f"{k}={arms[k]}" for k in ("h0", "h1", "h2", "h3")))
print(f"short_run_s={noise['short_run_s']}")
print(f"entry_slack={noise['entry_slack']}")
print(f"band_percentile={noise['band_percentile']}")
print("scaling_threads=" + ",".join(str(t) for t in scaling["threads"]))
print(f"attribution_arm={scaling['attribution_arm']}")
print(f"attribution_threads={scaling['attribution_threads']}")
print(f"gph_version={comp['gph_version']}")
print(f"controller_cpu={pin['controller_cpu']}")

# The competitor builds a complete study needs. "if-odd-prime" requires the
# build when an entry of this corpus names an odd modulus.
odd_prime = any(
    entry.get("modulus", 2) % 2 == 1 for entry in corpus.get("entry", [])
)
needed = []
for key, name in (
    ("ripser", "ripser"),
    ("ripser_coeff", "ripser-coeff"),
    ("ripser_f64", "ripser-f64"),
):
    rule = required[key]
    if rule == "if-odd-prime":
        rule = odd_prime
    if not isinstance(rule, bool):
        sys.exit(f"[meta.required_competitors].{key} must be true, false, "
                 f"or \"if-odd-prime\"")
    if rule:
        needed.append(name)
print("required_competitors=" + ",".join(needed))
EOF
}

# corpus_rows TABLE : one tab-separated row per entry of [[entry]] or
# [[scaling]], fields in the order the loops below read them. It fails on a
# missing field, an unknown field, or a duplicate id, so a corpus edit
# cannot silently drop an axis.
corpus_rows() {
    python3 - "$CORPUS" "$1" <<'EOF'
import sys
import tomllib

path, table = sys.argv[1], sys.argv[2]
with open(path, "rb") as f:
    corpus = tomllib.load(f)
rows = corpus.get(table, [])
if not rows:
    sys.exit(f"corpus has no [[{table}]] entries")

COMMON = ("id", "input", "n", "max_dim", "tau", "modulus", "seed")
ENTRY_ONLY = ("stratum", "headline", "competitor")
BY_INPUT = {"cloud": ("family", "coord_dim"), "graph": ("generator", "param")}
OPTIONAL = ("collapse", "multicore")
ORDER = ("id", "stratum", "input", "family", "generator", "n", "coord_dim",
         "param", "max_dim", "tau", "modulus", "seed", "headline",
         "competitor", "collapse", "multicore")
COMPETITORS = ("ripser", "ripser-coeff", "none")

seen = set()
for entry in rows:
    name = entry.get("id", "?")
    kind = entry.get("input")
    if kind not in BY_INPUT:
        sys.exit(f"entry {name}: input must be cloud or graph, got {kind!r}")
    required = COMMON + BY_INPUT[kind]
    if table == "entry":
        required += ENTRY_ONLY
    missing = [k for k in required if k not in entry]
    if missing:
        sys.exit(f"entry {name} lacks: {', '.join(missing)}")
    extra = set(entry) - set(required) - set(OPTIONAL)
    if extra:
        sys.exit(f"entry {name} carries unknown fields: {', '.join(sorted(extra))}")
    if table == "entry" and entry["competitor"] not in COMPETITORS:
        sys.exit(f"entry {name}: competitor must be one of {', '.join(COMPETITORS)}")
    if entry["id"] in seen:
        sys.exit(f"duplicate entry id {entry['id']}")
    seen.add(entry["id"])
    row = {
        "stratum": "cpu-scaling",
        "family": "-",
        "generator": "-",
        "coord_dim": 0,
        "param": 0,
        "headline": True,
        "competitor": "ripser",
        "collapse": "yes" if entry.get("collapse", False) else "no",
        "multicore": "yes" if entry.get("multicore", True) else "no",
    }
    row.update({k: entry[k] for k in required})
    row["headline"] = "yes" if row["headline"] else "no"
    print("\t".join(str(row[k]) for k in ORDER))
EOF
}

# route_decision N EDGES FORMAT : the engine the frozen H3 routing rule
# selects, and the two tests behind it, as one key=value line. A sparse
# input file is a graph already and takes the sparse engine whatever the
# counts say. The constants are the ones in crates/holos-tda/src/lib.rs at
# H3: N_MIN 32, the density cutoff four fifths, and a conversion of 24 bytes
# an edge plus 24 a point plus 8 against a budget of the larger of 32 MiB
# and the bytes of the compact matrix (8 a pair).
#
# route_decision --constants prints those constants as one key=value line.
# The record and the manifest carry it, and Amendment 2 of the corpus audits
# it against the routing rule at H3.
route_decision() {
    python3 - "$1" "${2:--}" "${3:--}" <<'EOF'
import sys

N_MIN = 32
DENSITY_NUM, DENSITY_DEN = 5, 4
BYTES_PER_EDGE = 24
BYTES_PER_POINT = 24
BYTES_FIXED = 8
BUDGET_FLOOR = 32 * 1024 * 1024
BYTES_PER_PAIR = 8

if sys.argv[1] == "--constants":
    print(f"n_min={N_MIN} "
          f"density={DENSITY_NUM}m<={DENSITY_DEN}*C(n,2) "
          f"memory={BYTES_PER_EDGE}m+{BYTES_PER_POINT}n+{BYTES_FIXED}"
          f"<=max({BUDGET_FLOOR},C(n,2)*{BYTES_PER_PAIR}) "
          f"sparse-input=always-sparse-selected")
    raise SystemExit

n, edges, fmt = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
pairs = n * (n - 1) // 2
if fmt == "sparse":
    print(f"engine=sparse selected=sparse-selected reason=sparse-input "
          f"pairs={pairs} density_ok=n/a memory_ok=n/a")
    raise SystemExit
density_ok = DENSITY_NUM * edges <= DENSITY_DEN * pairs
budget = max(BUDGET_FLOOR, pairs * BYTES_PER_PAIR)
conversion = BYTES_PER_EDGE * edges + BYTES_PER_POINT * n + BYTES_FIXED
memory_ok = conversion <= budget
routes = n >= N_MIN and density_ok and memory_ok
print(f"engine={'sparse' if routes else 'dense'} "
      f"selected={'sparse-selected' if routes else 'dense-selected'} "
      f"reason={'routed' if routes else 'kept-matrix'} pairs={pairs} "
      f"density_ok={'yes' if density_ok else 'no'} "
      f"memory_ok={'yes' if memory_ok else 'no'} budget_bytes={budget} "
      f"conversion_bytes={conversion}")
EOF
}

# physical_cores_of LIST : the number of distinct physical cores the
# comma-separated logical CPU list covers, or "unknown". A core is a socket
# and core id pair, because core ids repeat across sockets.
physical_cores_of() {
    lscpu -p=CPU,CORE,SOCKET 2>/dev/null | awk -F, -v want="$1" '
        BEGIN {
            n = split(want, parts, ",")
            for (i = 1; i <= n; i++) {
                if (split(parts[i], r, "-") == 2) { for (c = r[1]; c <= r[2]; c++) ok[c] = 1 }
                else { ok[parts[i]] = 1 }
            }
        }
        /^[0-9]/ { if ($1 in ok) core[$3 "." $2] = 1 }
        END { n = 0; for (c in core) n++; if (n == 0) exit 1; print n }
    ' || echo unknown
}

# siblings_of CPU : the logical CPUs that share a physical core with CPU,
# one per line, from sysfs. It fails when the kernel exports no sibling list.
siblings_of() {
    local file="/sys/devices/system/cpu/cpu$1/topology/thread_siblings_list"
    [[ -r "$file" ]] || return 1
    cpus_of "$(cat "$file")"
}

# timed_cpus : every logical CPU of the frozen pinning lists, sorted, one per
# line. A timed run uses one of these and nothing else.
timed_cpus() {
    {
        cpus_of "$CORES_1"
        cpus_of "$CORES_2"
        cpus_of "$CORES_4"
    } | sort -n -u
}

# cpus_of LIST : the logical CPUs of a comma-separated list, one per line.
cpus_of() {
    awk -v want="$1" 'BEGIN {
        n = split(want, parts, ",")
        for (i = 1; i <= n; i++) {
            if (split(parts[i], r, "-") == 2) { for (c = r[1]; c <= r[2]; c++) print c }
            else { print parts[i] }
        }
    }'
}

ratio() {
    awk -v a="${1:-}" -v b="${2:-}" 'BEGIN {
        if (a + 0 == 0 || b + 0 == 0) print "n/a"; else printf "%.3f", a / b
    }'
}

or_na() {
    if [[ -n "${1:-}" ]]; then echo "$1"; else echo "n/a"; fi
}

# rss_kb STATS : the peak RSS of one run in kB, empty when the sampler
# caught nothing. measure.py prints 0 for a miss, and 0 is never a reading.
rss_kb() {
    local kb
    kb="$(field max_rss_kb "${1:-}")"
    if [[ -n "$kb" && "$kb" != 0 ]]; then
        echo "$kb"
    fi
}

# record_cmd CMD... : the command as the record shows it. The provenance
# rule in _common.sh forbids an absolute path, a home directory, or a
# username in a record.
record_cmd() {
    local text="$*"
    for name in "${!COMPETITOR_BIN[@]}"; do
        text="${text//${COMPETITOR_BIN[$name]}/$name}"
    done
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        text="${text//$CARGO_TARGET_DIR/<CARGO_TARGET_DIR>}"
    fi
    text="${text//$HIST_DIR/<HIST_DIR>}"
    echo "${text//$ROOT\//}"
}

SETTINGS="$(corpus_settings)"
settings_field() { grep -m1 "^$1=" <<<"$SETTINGS" | cut -d= -f2-; }
CORPUS_VERSION="$(settings_field version)"
CORPUS_DATE="$(settings_field date)"
PHYSICAL_CORES="$(settings_field physical_cores)"
CORES_1="$(settings_field cores_1)"
CORES_2="$(settings_field cores_2)"
CORES_4="$(settings_field cores_4)"
ARMS_DEFAULT="$(settings_field arms)"
SHORT_RUN_S="$(settings_field short_run_s)"
ENTRY_SLACK="$(settings_field entry_slack)"
BAND_PERCENTILE="$(settings_field band_percentile)"
SCALING_THREADS="$(settings_field scaling_threads)"
ATTRIBUTION_ARM="$(settings_field attribution_arm)"
ATTRIBUTION_THREADS="$(settings_field attribution_threads)"
GPH_WANT_VERSION="$(settings_field gph_version)"
CONTROLLER_CPU="${NS_CONTROLLER_CPU:-$(settings_field controller_cpu)}"
REQUIRED_COMPETITORS="$(settings_field required_competitors)"
ROUTING_RULE="$(route_decision --constants)"
HOLOS_ARMS="${HOLOS_ARMS:-$ARMS_DEFAULT}"

# The arms of a complete study are the frozen ones of the corpus. A hand
# written HOLOS_ARMS runs, and the record says the arms are not frozen.
ARMS_FROZEN=yes
if [[ "$HOLOS_ARMS" != "$ARMS_DEFAULT" || "$ARMS_DEFAULT" == *=unset* ]]; then
    ARMS_FROZEN=no
fi

DRAFT=0
if ((CORPUS_VERSION == 0)); then
    if [[ "${ALLOW_DRAFT:-}" != "1" ]]; then
        echo "error: $(basename "$CORPUS") is version 0 and not frozen" >&2
        echo "       freeze it, or set ALLOW_DRAFT=1 to run it void" >&2
        exit 1
    fi
    DRAFT=1
fi

for spec in $HOLOS_ARMS; do
    if [[ "${spec#*=}" == unset ]]; then
        echo "error: arm ${spec%%=*} is unset in the corpus" >&2
        echo "       fill [meta.arm_commits] at the release freeze, by amendment," >&2
        echo "       or name the arms yourself with HOLOS_ARMS=label=commit ..." >&2
        exit 1
    fi
done
if [[ " $HOLOS_ARMS " != *" h3="* ]]; then
    echo "error: the arms name no h3; every grade and every agreement pass reads it" >&2
    exit 1
fi

# The competitor builds a complete study needs, from
# [meta.required_competitors] of the corpus. The odd-prime build is required
# when an entry names an odd modulus.
competitor_env() {
    case "$1" in
        ripser) echo RIPSER_BIN ;;
        ripser-coeff) echo RIPSER_COEFF_BIN ;;
        ripser-f64) echo RIPSER_F64_BIN ;;
        ripser-coeff-f64) echo RIPSER_COEFF_F64_BIN ;;
        *) echo "" ;;
    esac
}
MISSING_COMPETITORS=""
for name in ${REQUIRED_COMPETITORS//,/ }; do
    var="$(competitor_env "$name")"
    if [[ -z "$var" ]]; then
        echo "error: the corpus requires an unknown competitor: $name" >&2
        exit 1
    fi
    if [[ -z "${!var:-}" ]]; then
        MISSING_COMPETITORS="$MISSING_COMPETITORS${MISSING_COMPETITORS:+ }$name"
    fi
done
if [[ -n "$MISSING_COMPETITORS" && "${ALLOW_MISSING_COMPETITORS:-}" != "1" ]]; then
    echo "error: the corpus requires these competitor builds, and none was given:" >&2
    echo "       $MISSING_COMPETITORS" >&2
    echo "       point RIPSER_BIN, RIPSER_COEFF_BIN, or RIPSER_F64_BIN at them, or" >&2
    echo "       set ALLOW_MISSING_COMPETITORS=1 to run an incomplete study" >&2
    exit 1
fi

# The pinning rule. Registered runs need a known topology, every listed CPU
# inside the allowed set, and one logical CPU per physical core in each
# list. ALLOW_ANY_TOPOLOGY=1 overrides the refusal and voids the run.
TOPOLOGY_VOID=0
TOPOLOGY_PROBLEM=""
ALLOWED="${NS_ALLOWED_CPUS:-$( (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown)}"

# check_no_smt LIST : true when no two CPUs of LIST share a physical core,
# which is the one logical CPU per physical core the corpus states. It reads
# the kernel's sibling lists, so it does not depend on lscpu.
SMT_PROBLEM=""
check_no_smt() {
    local list="$1" cpu other sib
    while read -r cpu; do
        if ! sib="$(siblings_of "$cpu")"; then
            SMT_PROBLEM="the kernel exports no thread siblings for CPU $cpu"
            return 1
        fi
        while read -r other; do
            if [[ "$other" != "$cpu" ]] && grep -qx "$other" <<<"$sib"; then
                SMT_PROBLEM="CPUs $cpu and $other of pinning list $list share a physical core"
                return 1
            fi
        done < <(cpus_of "$list")
    done < <(cpus_of "$list")
    return 0
}

check_pinning() {
    local list cores want cpu
    if [[ "$ALLOWED" == unknown ]]; then
        TOPOLOGY_PROBLEM="the allowed CPU set is unknown"
        return 1
    fi
    for list in "$CORES_1:1" "$CORES_2:2" "$CORES_4:$PHYSICAL_CORES"; do
        want="${list##*:}"
        list="${list%:*}"
        while read -r cpu; do
            if ! grep -qx "$cpu" <(cpus_of "$ALLOWED"); then
                TOPOLOGY_PROBLEM="pinning list $list holds CPU $cpu, outside the allowed set $ALLOWED"
                return 1
            fi
        done < <(cpus_of "$list")
        cores="$(physical_cores_of "$list")"
        if [[ "$cores" == unknown ]]; then
            TOPOLOGY_PROBLEM="the CPU topology is unknown, so pinning list $list cannot be checked"
            return 1
        fi
        if ((cores != want)) || (($(cpus_of "$list" | wc -l) != want)); then
            TOPOLOGY_PROBLEM="pinning list $list is not $want physical cores without SMT ($cores cores over $(cpus_of "$list" | wc -l) logical CPUs)"
            return 1
        fi
        if ! check_no_smt "$list"; then
            TOPOLOGY_PROBLEM="$SMT_PROBLEM"
            return 1
        fi
    done
    return 0
}
TOPOLOGY_CHECK="ok: one logical CPU per physical core in every pinning list, from thread_siblings_list"
if ! check_pinning; then
    TOPOLOGY_CHECK="failed: $TOPOLOGY_PROBLEM"
    if [[ "${ALLOW_ANY_TOPOLOGY:-}" != "1" ]]; then
        echo "error: $TOPOLOGY_PROBLEM" >&2
        echo "       the corpus [meta.pinning] rule is part of the registration;" >&2
        echo "       fix the pinning, or set ALLOW_ANY_TOPOLOGY=1 to run it void" >&2
        exit 1
    fi
    TOPOLOGY_VOID=1
fi

# The measurement controller. Amendment 1 of the corpus puts the runner's own
# process, measure.py, and the peak RSS sampler inside it on one logical CPU
# outside every timed physical core. There is no override: a controller that
# shares a core with a timed run changes every number below it.
CONTROLLER_PROBLEM=""
check_controller() {
    local cpu sib
    if [[ ! "$CONTROLLER_CPU" =~ ^[0-9]+$ ]]; then
        CONTROLLER_PROBLEM="the controller CPU '$CONTROLLER_CPU' is not a logical CPU number"
        return 1
    fi
    if [[ ! -d "/sys/devices/system/cpu/cpu$CONTROLLER_CPU" ]]; then
        CONTROLLER_PROBLEM="this machine has no logical CPU $CONTROLLER_CPU"
        return 1
    fi
    while read -r cpu; do
        if [[ "$cpu" == "$CONTROLLER_CPU" ]]; then
            CONTROLLER_PROBLEM="controller CPU $CONTROLLER_CPU is a timed CPU"
            return 1
        fi
        if ! sib="$(siblings_of "$cpu")"; then
            CONTROLLER_PROBLEM="the kernel exports no thread siblings for timed CPU $cpu, so the controller placement cannot be checked"
            return 1
        fi
        if grep -qx "$CONTROLLER_CPU" <<<"$sib"; then
            CONTROLLER_PROBLEM="controller CPU $CONTROLLER_CPU shares a physical core with timed CPU $cpu"
            return 1
        fi
    done < <(timed_cpus)
    return 0
}
if ! check_controller; then
    echo "error: $CONTROLLER_PROBLEM" >&2
    echo "       the controller runs the runner, measure.py, and the peak RSS sampler;" >&2
    echo "       Amendment 1 of the corpus keeps it off every timed physical core." >&2
    echo "       Set NS_CONTROLLER_CPU to a logical CPU outside $(timed_cpus | paste -sd, -) and their SMT siblings." >&2
    exit 1
fi
CONTROLLER_CHECK="CPU $CONTROLLER_CPU shares no physical core with the timed CPUs $(timed_cpus | paste -sd, -)"

# The runner re-execs itself on the controller CPU. Timed children set their
# own affinity, through MEASURE_AFFINITY in measure.py and through taskset
# for giotto-ph, so none of them inherits this pin.
if [[ "${NS_CONTROLLER_ACTIVE:-}" != "1" ]]; then
    export NS_CONTROLLER_ACTIVE=1
    export NS_ALLOWED_CPUS="$ALLOWED"
    echo "measurement controller: CPU $CONTROLLER_CPU; timed CPUs $(timed_cpus | paste -sd, -)" >&2
    exec taskset -c "$CONTROLLER_CPU" "${BASH:-bash}" "$HERE/$(basename "$0")" "$@"
fi
CONTROLLER_ACTUAL="$( (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown)"
if [[ "$CONTROLLER_ACTUAL" != "$CONTROLLER_CPU" ]]; then
    echo "error: the controller process runs on CPUs $CONTROLLER_ACTUAL, not on CPU $CONTROLLER_CPU alone" >&2
    exit 1
fi

# The record names the allowed CPU set of the study, not the controller's one
# core. The controller sets its own affinity above.
PROV_AFFINITY_OVERRIDE="$ALLOWED"

# on_allowed CMD... : run one untimed command on the allowed CPU set instead
# of the controller CPU. The arm builds and the input generation are untimed,
# and one core would cost hours.
on_allowed() {
    if [[ "$ALLOWED" == unknown ]]; then
        "$@"
    else
        taskset -c "$ALLOWED" "$@"
    fi
}

# giotto-ph. The multicore grade and the CPU scaling table read it, so its
# absence is a refusal unless the caller accepts a run without them.
GPH_VERSION="$("$GPH_PYTHON" -c 'import gph; print(getattr(gph, "__version__", "unknown"))' 2>/dev/null || echo absent)"
GPH_VOID=0
HAVE_GPH=1
if [[ "$GPH_VERSION" == absent ]]; then
    HAVE_GPH=0
    if [[ "${ALLOW_NO_GPH:-}" != "1" ]]; then
        echo "error: $GPH_PYTHON does not import gph, so the multicore grade cannot run" >&2
        echo "       install giotto-ph $GPH_WANT_VERSION, point GPH_PYTHON at the" >&2
        echo "       interpreter that has it, or set ALLOW_NO_GPH=1 to run without it" >&2
        exit 1
    fi
elif [[ "$GPH_VERSION" != "$GPH_WANT_VERSION" ]]; then
    if [[ "${ALLOW_ANY_GPH_VERSION:-}" != "1" ]]; then
        echo "error: giotto-ph $GPH_VERSION is installed, and the corpus names $GPH_WANT_VERSION" >&2
        echo "       install the registered version, or set ALLOW_ANY_GPH_VERSION=1 to void" >&2
        echo "       the multicore grade" >&2
        exit 1
    fi
    GPH_VOID=1
fi

# build_tree : release build of the working tree's holos binary and of the
# driver that performs the untimed edge collapse of a collapsed entry.
build_tree() {
    on_allowed "${CARGO_CMD[@]}" build --release -p holos-tda -p engine-bench \
        --manifest-path "$ROOT/Cargo.toml" >&2
    local dir="${CARGO_TARGET_DIR:-$ROOT/target}"
    HOLOS_BIN="$dir/release/holos"
    HOLOS_BIN_DISPLAY="target/release/holos"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        HOLOS_BIN_DISPLAY='<CARGO_TARGET_DIR>/release/holos'
    fi
    BUILD_CMD_DISPLAY="$CARGO build --release -p holos-tda -p engine-bench"
    COLLAPSE_BIN="$dir/release/engine-bench"
}

# resolve_arms : one holos build per HOLOS_ARMS item, in the order given. A
# historical arm gets a detached checkout of its commit under
# HIST_DIR/<sha>/ and is built there, into that checkout's own target
# directory, so no arm can overwrite another's artifacts.
resolve_arms() {
    ARM_LABEL=()
    ARM_BIN=()
    ARM_COMMIT=()
    ARM_SHA=()
    ARM_VERSION=()
    local spec label commit sha dir bin
    local -a specs
    read -r -a specs <<<"$HOLOS_ARMS"
    for spec in "${specs[@]}"; do
        if [[ "$spec" != *=* ]]; then
            echo "error: HOLOS_ARMS item '$spec' is not label=commit" >&2
            exit 1
        fi
        label="${spec%%=*}"
        commit="${spec#*=}"
        if [[ -z "$label" || "$label" == *[^a-zA-Z0-9._-]* ]]; then
            echo "error: arm label '$label' must be letters, digits, dot, dash, or underscore" >&2
            exit 1
        fi
        case "$label" in
            h3aa | ripser | ripser-coeff | ripser-f64 | ripser-coeff-f64 | gph)
                echo "error: arm label '$label' is reserved for the record" >&2
                exit 1
                ;;
        esac
        if [[ "$commit" == "@" ]]; then
            ARM_LABEL+=("$label")
            ARM_BIN+=("$HOLOS_BIN")
            ARM_COMMIT+=("$(head_commit)")
        else
            if ! sha="$(git -C "$ROOT" rev-parse --verify --quiet "$commit^{commit}")"; then
                echo "error: arm $label names no commit: $commit" >&2
                exit 1
            fi
            dir="$HIST_DIR/${sha:0:12}"
            bin="$dir/target/release/holos"
            if [[ ! -x "$bin" || "${REBUILD_HIST:-}" == "1" ]]; then
                if [[ ! -d "$dir" ]]; then
                    git -C "$ROOT" worktree add --detach "$dir" "$sha" >&2
                fi
                on_allowed "${CARGO_CMD[@]}" build --release -p holos-tda \
                    --manifest-path "$dir/Cargo.toml" --target-dir "$dir/target" >&2
            fi
            if [[ ! -x "$bin" ]]; then
                echo "error: arm $label built no binary for commit ${sha:0:12}" >&2
                exit 1
            fi
            ARM_LABEL+=("$label")
            ARM_BIN+=("$bin")
            ARM_COMMIT+=("$sha")
        fi
        ARM_SHA+=("$(sha256 "${ARM_BIN[-1]}")")
        ARM_VERSION+=("$("${ARM_BIN[-1]}" --version)")
    done
}

mkdir -p "$DATA" "$DATA/aa"
build_tree
resolve_arms

# The A/A control. The copy must hash the same as the arm it copies, or it
# is a second build and measures something else.
H3_BIN=""
H3_SHA=""
for i in "${!ARM_LABEL[@]}"; do
    if [[ "${ARM_LABEL[$i]}" == h3 ]]; then
        H3_BIN="${ARM_BIN[$i]}"
        H3_SHA="${ARM_SHA[$i]}"
    fi
done
AA_BIN="$DATA/aa/holos-h3-aa"
cp -f "$H3_BIN" "$AA_BIN"
chmod +x "$AA_BIN"
AA_SHA="$(sha256 "$AA_BIN")"
if [[ "$AA_SHA" != "$H3_SHA" ]]; then
    echo "error: the A/A copy hashes $AA_SHA and h3 hashes $H3_SHA" >&2
    echo "       the control must be the same bytes, or it is a second build" >&2
    exit 1
fi

declare -A BIN_OF=()
declare -A KIND_OF=()
declare -A COMPETITOR_BIN=()
for i in "${!ARM_LABEL[@]}"; do
    BIN_OF["${ARM_LABEL[$i]}"]="${ARM_BIN[$i]}"
    KIND_OF["${ARM_LABEL[$i]}"]=holos
done
BIN_OF[h3aa]="$AA_BIN"
KIND_OF[h3aa]=holos
for pair in "ripser:${RIPSER_BIN:-}" "ripser-coeff:${RIPSER_COEFF_BIN:-}" \
    "ripser-f64:${RIPSER_F64_BIN:-}" "ripser-coeff-f64:${RIPSER_COEFF_F64_BIN:-}"; do
    name="${pair%%:*}"
    path="${pair#*:}"
    if [[ -n "$path" ]]; then
        BIN_OF["$name"]="$path"
        COMPETITOR_BIN["$name"]="$path"
        case "$name" in
            *coeff*) KIND_OF["$name"]=ripser-coeff ;;
            *) KIND_OF["$name"]=ripser ;;
        esac
    fi
done
KIND_OF[gph]=gph

# build_cmd LABEL THREADS : the argv of one command, in CMD. Every holos
# arm runs one command line: no arm is forced to an engine or a storage
# form, so each build takes the path it ships.
build_cmd() {
    local label="$1" threads="$2"
    case "${KIND_OF[$label]}" in
        holos)
            CMD=("${BIN_OF[$label]}" --format "$e_format" --dim "$e_max_dim"
                --threshold "$e_threshold" --modulus "$e_modulus"
                --threads "$threads" --output ripser "$e_file")
            ;;
        ripser)
            CMD=("${BIN_OF[$label]}" --format "$e_format" --dim "$e_max_dim"
                --threshold "$e_threshold" "$e_file")
            ;;
        ripser-coeff)
            CMD=("${BIN_OF[$label]}" --format "$e_format" --dim "$e_max_dim"
                --threshold "$e_threshold" --modulus "$e_modulus" "$e_file")
            ;;
        *)
            echo "error: label $label has no command" >&2
            exit 1
            ;;
    esac
}

# run_gph THREADS CORES OUT : one timed giotto-ph run, pinned to CORES,
# writing a ripser-format diagram to OUT and printing "wall_s=<s>". The
# clock wraps ripser_parallel alone, as benchmarks/giotto_compare.sh does,
# so it excludes process start and input parse. A dense entry is read into
# a square matrix, a graph entry into a sparse one; both go in as
# precomputed distances, which is the same geometry holos and ripser read.
run_gph() {
    taskset -c "$2" "$GPH_PYTHON" - "$e_file" "$e_format" "$e_n" "$e_threshold" \
        "$e_max_dim" "$e_modulus" "$1" "$3" <<'EOF'
import sys
import time

import numpy as np
from gph import ripser_parallel

path, fmt, n, thresh, maxdim, modulus, threads, out = (
    sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4]),
    int(sys.argv[5]), int(sys.argv[6]), int(sys.argv[7]), sys.argv[8],
)

if fmt == "sparse":
    from scipy.sparse import coo_matrix

    rows, cols, data = [], [], []
    with open(path) as f:
        for line in f:
            i, j, d = line.split()
            rows.append(int(i))
            cols.append(int(j))
            data.append(float(d))
    # A zero-weight edge would be an explicit zero in the sparse matrix,
    # which scipy may drop. No graph entry of this corpus draws one.
    matrix = coo_matrix((data, (rows, cols)), shape=(n, n))
else:
    # The condensed lower triangle, row by row, is exactly the order
    # np.tril_indices produces, so one conversion fills the matrix.
    with open(path) as f:
        values = np.array(f.read().split(), dtype=np.float64)
    matrix = np.zeros((n, n), dtype=np.float64)
    matrix[np.tril_indices(n, -1)] = values
    matrix += matrix.T

start = time.monotonic()
res = ripser_parallel(
    matrix, metric="precomputed", maxdim=maxdim, thresh=thresh,
    coeff=modulus, n_threads=threads,
)
wall = time.monotonic() - start

with open(out, "w") as f:
    for dim, dgm in enumerate(res["dgms"]):
        f.write(f"persistence intervals in dim {dim}:\n")
        for birth, death in dgm:
            d = "" if death == float("inf") else repr(float(death))
            f.write(f" [{repr(float(birth))},{d})\n")

print(f"wall_s={wall:.6f}")
EOF
}

# run_one LABEL THREADS CORES OUT ERR : one run of one command, pinned to
# CORES, printing "wall_s=<s> max_rss_kb=<kb>". A fresh process is timed by
# measure.py, which pins the child itself so that argv[0] stays the target
# binary and the peak RSS sampler keeps working. giotto-ph is timed in
# process and reports no RSS.
run_one() {
    local label="$1" threads="$2" cores="$3" out="$4" err="$5" stats
    if [[ "${KIND_OF[$label]}" == gph ]]; then
        stats="$(run_gph "$threads" "$cores" "$out" 2>"$err")" || return 1
        echo "$stats max_rss_kb=0"
        return 0
    fi
    build_cmd "$label" "$threads"
    MEASURE_AFFINITY="$cores" measure_err "$out" "$err" "${CMD[@]}"
}

# spec_field SPEC INDEX : one field of a "label:threads:cores" spec.
spec_field() {
    awk -F: -v i="$2" '{ print $i }' <<<"$1"
}

# time_pass ID PASS SPEC... : the agreement pass and then the rotated timed
# repetitions for one entry and one pass. Each spec is
# "label:threads:cores". The first spec must be an h3 spec: its warm-up run
# writes the reference diagram when the entry has none yet, and every other
# command is compared against that reference.
#
# One repetition runs every spec once, starting one place further on than
# the last, so no command keeps the first position. The repetition count is
# the smallest multiple of the spec count at or above REPS. It appends one
# row per spec to TOTALS and returns 1 when the entry is void.
time_pass() {
    local id="$1" pass="$2"
    shift 2
    local -a specs=("$@")
    local count="${#specs[@]}" label threads cores out err stats wall rss reps r i spec walls
    local -A PEAK_RSS=()
    if [[ "$(spec_field "${specs[0]}" 1)" != h3 ]]; then
        echo "error: the first spec of a pass must be h3, got ${specs[0]}" >&2
        exit 1
    fi
    reps=$(((REPS + count - 1) / count * count))

    {
        echo "-- pass=$pass entry=$id commands=$count repetitions=$reps balanced=yes"
    } >>"$RESULTS"

    local ref="$DATA/ns_${id}_ref.out"
    for spec in "${specs[@]}"; do
        label="$(spec_field "$spec" 1)"
        threads="$(spec_field "$spec" 2)"
        cores="$(spec_field "$spec" 3)"
        out="$DATA/ns_${id}_${pass}_${label}_t${threads}.out"
        err="$DATA/ns_${id}_${pass}_${label}_t${threads}.err"
        if [[ "${KIND_OF[$label]}" != gph ]]; then
            build_cmd "$label" "$threads"
            echo "cmd ($pass, $label, $threads threads, cpus $cores): $(record_cmd "${CMD[@]}")" >>"$RESULTS"
        else
            echo "cmd ($pass, gph, $threads threads, cpus $cores): in-process ripser_parallel, giotto-ph $GPH_VERSION" >>"$RESULTS"
        fi
        if ! stats="$(run_one "$label" "$threads" "$cores" "$out" "$err")"; then
            {
                echo "VOID: $label exited nonzero on its warm-up run"
                sed 's/^/  /' "$err"
            } >>"$RESULTS"
            return 1
        fi
        if [[ "$label" == h3 && ! -s "$ref" ]]; then
            cp -f "$out" "$ref"
            continue
        fi
        if [[ "$(compare_diagrams "$ref" "$out")" != yes ]]; then
            echo "VOID: $label disagrees with the h3 reference diagram" >>"$RESULTS"
            return 1
        fi
    done

    for spec in "${specs[@]}"; do
        label="$(spec_field "$spec" 1)"
        threads="$(spec_field "$spec" 2)"
        : >"$DATA/ns_walls_${id}_${pass}_${label}_t${threads}.txt"
    done

    for ((r = 0; r < reps; r++)); do
        for ((i = 0; i < count; i++)); do
            spec="${specs[$(((i + r) % count))]}"
            label="$(spec_field "$spec" 1)"
            threads="$(spec_field "$spec" 2)"
            cores="$(spec_field "$spec" 3)"
            out="$DATA/ns_${id}_${pass}_${label}_t${threads}.out"
            err="$DATA/ns_${id}_${pass}_${label}_t${threads}.err"
            if ! stats="$(run_one "$label" "$threads" "$cores" "$out" "$err")"; then
                {
                    echo "VOID: $label exited nonzero on repetition $((r + 1))"
                    sed 's/^/  /' "$err"
                } >>"$RESULTS"
                return 1
            fi
            wall="$(field wall_s "$stats")"
            echo "$wall" >>"$DATA/ns_walls_${id}_${pass}_${label}_t${threads}.txt"
            rss="$(rss_kb "$stats")"
            if [[ -n "$rss" ]] && ((rss > ${PEAK_RSS["$label:$threads"]:-0})); then
                PEAK_RSS["$label:$threads"]="$rss"
            fi
        done
    done

    for spec in "${specs[@]}"; do
        label="$(spec_field "$spec" 1)"
        threads="$(spec_field "$spec" 2)"
        walls="$DATA/ns_walls_${id}_${pass}_${label}_t${threads}.txt"
        stats="$(median_iqr <"$walls")"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$pass" "$label" "$threads" \
            "$(field median "$stats")" "$(field iqr "$stats")" \
            "$(or_na "${PEAK_RSS["$label:$threads"]:-}")" >>"$TOTALS"
        echo "id=$id pass=$pass $label t$threads median=$(field median "$stats")s" >&2
    done
    return 0
}

HEADER="holos north-star engine study, registered
The public performance statements of this release come from this record and
from nothing else."
if ((DRAFT != 0)); then
    HEADER="DRAFT-CORPUS: $(basename "$CORPUS") is version 0 and not frozen.
Its numbers are void, and no claim may cite them.
$HEADER"
fi
if ((TOPOLOGY_VOID != 0)); then
    HEADER="TOPOLOGY-UNVERIFIED: $TOPOLOGY_PROBLEM.
Its numbers are void, and no claim may cite them.
$HEADER"
fi
if [[ "$NS_ONLY" != "*" ]]; then
    HEADER="FILTERED: NS_ONLY=$NS_ONLY ran part of the corpus, so this run grades
nothing.
$HEADER"
fi
if ((GPH_VOID != 0)); then
    HEADER="GPH-VERSION-UNREGISTERED: giotto-ph $GPH_VERSION is not the registered
$GPH_WANT_VERSION, so the multicore grade is void.
$HEADER"
fi
if ((HAVE_GPH == 0)); then
    HEADER="NO-GPH: giotto-ph is not installed, so the multicore pass and the CPU
scaling stratum did not run.
$HEADER"
fi
if [[ -n "$MISSING_COMPETITORS" ]]; then
    HEADER="MISSING-COMPETITOR: the corpus requires $MISSING_COMPETITORS, and the run
did not have it, so this is not a complete study.
$HEADER"
fi
if [[ "$ARMS_FROZEN" != yes ]]; then
    HEADER="ARMS-NOT-FROZEN: the arms are not the frozen ones of the corpus, so this
is not a complete study.
$HEADER"
fi

# The corpus hash before anything is generated. It is compared again before
# the first timed run, so an edit between the two cannot slip in.
CORPUS_SHA_AT_GENERATION="$(sha256 "$CORPUS")"

rm -f "$RESULTS_MD" "$MANIFEST"
emit_provenance "$RESULTS" "$HEADER"

RIPSER_DIR="$(cd "$(dirname "$RIPSER_BIN")" && pwd)"
if [[ -f "$RIPSER_DIR/Makefile" ]]; then
    RIPSER_FLAGS="$(awk '/^ripser:/ { getline; sub(/^\t+/, ""); print; exit }' "$RIPSER_DIR/Makefile") [from the Makefile beside the binary]"
else
    RIPSER_FLAGS="unknown (no Makefile beside the binary)"
fi
KERNEL="$(uname -sr)"

{
    echo "corpus: $(basename "$CORPUS") version $CORPUS_VERSION dated $CORPUS_DATE sha256 $CORPUS_SHA_AT_GENERATION"
    echo "entries filtered by: $NS_ONLY"
    for i in "${!ARM_LABEL[@]}"; do
        echo "arm ${ARM_LABEL[$i]}: commit ${ARM_COMMIT[$i]} sha256 ${ARM_SHA[$i]} version ${ARM_VERSION[$i]}"
    done
    echo "arm h3aa: the A/A control, a copy of h3, sha256 $AA_SHA"
    for name in ripser ripser-coeff ripser-f64 ripser-coeff-f64; do
        if [[ -n "${COMPETITOR_BIN[$name]:-}" ]]; then
            echo "competitor $name: $(basename "${COMPETITOR_BIN[$name]}") sha256 $(sha256 "${COMPETITOR_BIN[$name]}")"
        else
            echo "competitor $name: not given"
        fi
    done
    echo "ripser build: $RIPSER_FLAGS"
    echo "giotto-ph: $GPH_VERSION (registered $GPH_WANT_VERSION)"
    echo "pinning: 1 core $CORES_1; 2 cores $CORES_2; $PHYSICAL_CORES cores $CORES_4; allowed set $ALLOWED"
    echo "controller: the runner, measure.py, and its peak RSS sampler on CPU $CONTROLLER_CPU; $CONTROLLER_CHECK"
    echo "topology check: $TOPOLOGY_CHECK"
    echo "routing rule at h3: $ROUTING_RULE"
    echo "required competitors: $REQUIRED_COMPETITORS"
    echo "kernel: $KERNEL"
    echo "timing: fresh process per run for holos and ripser (measure.py, child pinned by MEASURE_AFFINITY); giotto-ph timed in process around ripser_parallel alone, which excludes its process start and its input parse"
    echo "repetitions: 1 warm-up, which is the agreement run, plus at least $REPS timed repetitions per command, rounded up per entry to a multiple of its command count and rotated"
    echo "agreement: every arm and every competitor against the h3 diagram, as interval multisets within $TOLERANCE"
    echo "noise rule: a ratio whose denominator median is under ${SHORT_RUN_S}s is descriptive and enters no median, no per-entry clause, no regression aggregate, and no band; the per-entry limit is max(1 + $ENTRY_SLACK, 1 + band), band = the ${BAND_PERCENTILE}th percentile of the A/A distances from 1.0 over the graded entries"
    echo "reporting: input strata first, then the grading strata, then the grades; an overall median never stands alone"
    echo
} >>"$RESULTS"

ENTRY_ROWS="$DATA/ns_entries.tsv"
SCALING_ROWS="$DATA/ns_scaling.tsv"
ENTRY_META="$DATA/ns_entry_meta.tsv"
SCALING_META="$DATA/ns_scaling_meta.tsv"
TOTALS="$DATA/ns_totals.tsv"
GENERATED="$DATA/ns_generated.tsv"
: >"$ENTRY_META"
: >"$SCALING_META"
: >"$TOTALS"
: >"$GENERATED"
corpus_rows entry >"$ENTRY_ROWS"
corpus_rows scaling >"$SCALING_ROWS"

selected() {
    local pattern
    for pattern in "${ONLY_PATTERNS[@]}"; do
        if [[ "$1" == $pattern ]]; then
            return 0
        fi
    done
    return 1
}
IFS=, read -r -a ONLY_PATTERNS <<<"$NS_ONLY"

# The generation pass. Every input is written and classified before any
# timing starts, so the routing classification and the timed runs read one
# set of files.
echo "== generation" >>"$RESULTS"
GENERATED_COUNT=0
generate_one() {
    local kind="$1" id="$2" stratum="$3" input="$4" family="$5" generator="$6"
    local n="$7" coord_dim="$8" param="$9" max_dim="${10}" tau="${11}"
    local modulus="${12}" seed="${13}" headline="${14}" competitor="${15}"
    local collapse="${16}" multicore="${17}"
    local cloud lower sparse graph threshold edges file format collapsed note

    # A stale reference from an earlier run must not survive into this one.
    rm -f "$DATA/ns_${id}_ref.out"

    cloud="$DATA/ns_${id}.csv"
    lower="$DATA/ns_${id}.lower"
    sparse="$DATA/ns_${id}.sparse"
    if [[ "$input" == cloud ]]; then
        on_allowed python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
        graph="$(on_allowed python3 "$HERE/densify_to_sparse.py" "$cloud" "$tau" "$sparse" "$lower")"
        file="$lower"
        format="lower-distance"
        note="cloud $family coord_dim=$coord_dim"
    else
        graph="$(on_allowed python3 "$HERE/gen_graph.py" "$generator" "$n" "$seed" "$tau" "$param" "$sparse")"
        file="$sparse"
        format="sparse"
        note="graph $generator param=$param"
    fi
    threshold="$(field threshold "$graph")"
    edges="$(field edges "$graph")"

    # A collapsed entry measures a real collapsed graph. The collapse runs
    # once, untimed, and its output replaces the input. The barcode does
    # not change.
    if [[ "$collapse" == yes ]]; then
        collapsed="$DATA/ns_${id}.collapsed"
        note="$note collapsed: $(on_allowed "$COLLAPSE_BIN" --entry "$id" --input "$sparse" \
            --format sparse --threshold "$threshold" --emit-collapsed "$collapsed")"
        file="$collapsed"
        format="sparse"
        edges="$(wc -l <"$collapsed")"
    fi

    local routing selected_stratum
    routing="$(route_decision "$n" "$edges" "$format")"
    selected_stratum="$(field selected "$routing")"

    {
        echo "== $kind id=$id stratum=$stratum"
        echo "source: $note n=$n tau=$tau max_dim=$max_dim modulus=$modulus seed=$seed"
        echo "input: $(basename "$file") format=$format threshold=$threshold edges=$edges"
        echo "routing at h3: $routing"
    } >>"$RESULTS"

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$kind" "$id" "$stratum" "$file" "$format" "$n" "$edges" "$threshold" \
        "$max_dim" "$modulus" "$headline" "$competitor" "$multicore" >>"$GENERATED"
    if [[ "$kind" == entry ]]; then
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$stratum" \
            "$selected_stratum" "$max_dim" "$n" "$edges" "$headline" "$competitor" "ok" \
            >>"$ENTRY_META"
    else
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$selected_stratum" "$max_dim" \
            "$n" "$edges" "ok" >>"$SCALING_META"
    fi
    GENERATED_COUNT=$((GENERATED_COUNT + 1))
}

while IFS=$'\t' read -r id stratum input family generator n coord_dim param max_dim tau modulus seed headline competitor collapse multicore; do
    selected "$id" || continue
    generate_one entry "$id" "$stratum" "$input" "$family" "$generator" "$n" \
        "$coord_dim" "$param" "$max_dim" "$tau" "$modulus" "$seed" "$headline" \
        "$competitor" "$collapse" "$multicore"
done <"$ENTRY_ROWS"

while IFS=$'\t' read -r id stratum input family generator n coord_dim param max_dim tau modulus seed headline competitor collapse multicore; do
    selected "$id" || continue
    if ((HAVE_GPH == 0)); then
        continue
    fi
    generate_one scaling "$id" "$stratum" "$input" "$family" "$generator" "$n" \
        "$coord_dim" "$param" "$max_dim" "$tau" "$modulus" "$seed" "$headline" \
        "$competitor" "$collapse" "$multicore"
done <"$SCALING_ROWS"

if ((GENERATED_COUNT == 0)); then
    echo "error: filter NS_ONLY=$NS_ONLY matched no entry" >&2
    exit 1
fi

# The freeze check. The corpus that named these inputs must be the corpus
# the timings are recorded against.
CORPUS_SHA_AT_TIMING="$(sha256 "$CORPUS")"
if [[ "$CORPUS_SHA_AT_TIMING" != "$CORPUS_SHA_AT_GENERATION" ]]; then
    echo "error: $(basename "$CORPUS") changed while the inputs were generated" >&2
    echo "       at generation: $CORPUS_SHA_AT_GENERATION" >&2
    echo "       now:           $CORPUS_SHA_AT_TIMING" >&2
    echo "       a corpus edit inside a run is not a registered run; start again" >&2
    exit 1
fi
echo "corpus sha256 unchanged between generation and timing: $CORPUS_SHA_AT_TIMING" >>"$RESULTS"
echo >>"$RESULTS"

ANY_VOID=0
DONE_IDS="$DATA/ns_done_ids.txt"
: >"$DONE_IDS"

# mark_void FILE ID : set the status field of one row to void, so the entry
# keeps its row and enters no median.
mark_void() {
    awk -F'\t' -v id="$2" 'BEGIN { OFS = "\t" } { if ($1 == id) $NF = "void"; print }' \
        "$1" >"$1.tmp" && mv "$1.tmp" "$1"
}

# serial_specs COMPETITOR : the commands of the serial pass, h3 first.
serial_specs() {
    local competitor="$1" label f64
    SPECS=("h3:1:$CORES_1")
    for label in "${ARM_LABEL[@]}"; do
        if [[ "$label" != h3 ]]; then
            SPECS+=("$label:1:$CORES_1")
        fi
    done
    SPECS+=("h3aa:1:$CORES_1")
    if [[ "$competitor" != none && -n "${BIN_OF[$competitor]:-}" ]]; then
        SPECS+=("$competitor:1:$CORES_1")
        f64="$competitor-f64"
        if [[ -n "${BIN_OF[$f64]:-}" ]]; then
            SPECS+=("$f64:1:$CORES_1")
        fi
    fi
}

while IFS=$'\t' read -r kind id stratum file format n edges threshold max_dim modulus headline competitor multicore; do
    [[ "$kind" == entry ]] || continue
    e_file="$file"
    e_format="$format"
    e_n="$n"
    e_threshold="$threshold"
    e_max_dim="$max_dim"
    e_modulus="$modulus"

    serial_specs "$competitor"
    if ! time_pass "$id" serial "${SPECS[@]}"; then
        ANY_VOID=1
        mark_void "$ENTRY_META" "$id"
        echo "id=$id VOID in the serial pass" >&2
        continue
    fi

    if [[ "$multicore" == yes ]] && ((HAVE_GPH == 1)); then
        if ! time_pass "$id" multicore "h3:$PHYSICAL_CORES:$CORES_4" \
            "h3aa:$PHYSICAL_CORES:$CORES_4" "gph:$PHYSICAL_CORES:$CORES_4"; then
            ANY_VOID=1
            mark_void "$ENTRY_META" "$id"
            echo "id=$id VOID in the multicore pass" >&2
            continue
        fi
    fi
    echo "entry=$id" >>"$DONE_IDS"
done <"$GENERATED"

# The CPU scaling stratum. Both products run at every core count of the
# frozen grid, and the attribution arm runs at the last one.
while IFS=$'\t' read -r kind id stratum file format n edges threshold max_dim modulus headline competitor multicore; do
    [[ "$kind" == scaling ]] || continue
    e_file="$file"
    e_format="$format"
    e_n="$n"
    e_threshold="$threshold"
    e_max_dim="$max_dim"
    e_modulus="$modulus"

    SPECS=("h3:1:$CORES_1" "h3aa:1:$CORES_1" "gph:1:$CORES_1")
    for t in ${SCALING_THREADS//,/ }; do
        case "$t" in
            1) continue ;;
            2) cores="$CORES_2" ;;
            *) cores="$CORES_4" ;;
        esac
        SPECS+=("h3:$t:$cores" "gph:$t:$cores")
    done
    if [[ -n "${BIN_OF[$ATTRIBUTION_ARM]:-}" ]]; then
        SPECS+=("$ATTRIBUTION_ARM:$ATTRIBUTION_THREADS:$CORES_4")
    fi
    if ! time_pass "$id" scaling "${SPECS[@]}"; then
        ANY_VOID=1
        mark_void "$SCALING_META" "$id"
        echo "id=$id VOID in the scaling pass" >&2
        continue
    fi
    echo "entry=$id" >>"$DONE_IDS"
done <"$GENERATED"

# The validity rule of the corpus. A complete study is a clean, unfiltered
# run of the frozen corpus with the frozen arms, every required competitor,
# the registered giotto-ph, a verified topology, and no void entry. Every
# reason is listed, not just the first.
CLEAN_TREE=yes
if [[ "$PROV_COMMIT" == *-DIRTY ]]; then
    CLEAN_TREE=no
fi
INVALID_REASONS=()
((ANY_VOID == 0)) || INVALID_REASONS+=("an entry was voided")
((DRAFT == 0)) || INVALID_REASONS+=("the corpus is an unfrozen draft")
((TOPOLOGY_VOID == 0)) || INVALID_REASONS+=("the topology is unverified: $TOPOLOGY_PROBLEM")
[[ "$NS_ONLY" == "*" ]] || INVALID_REASONS+=("NS_ONLY=$NS_ONLY ran part of the corpus")
[[ "$CLEAN_TREE" == yes ]] || INVALID_REASONS+=("the worktree was dirty")
((HAVE_GPH == 1)) || INVALID_REASONS+=("giotto-ph is not installed")
((GPH_VOID == 0)) || INVALID_REASONS+=("giotto-ph $GPH_VERSION is not the registered $GPH_WANT_VERSION")
[[ "$ARMS_FROZEN" == yes ]] || INVALID_REASONS+=("the arms are not the frozen ones of the corpus")
[[ -z "$MISSING_COMPETITORS" ]] || INVALID_REASONS+=("the run lacked a required competitor: $MISSING_COMPETITORS")
STUDY_VALID=yes
((${#INVALID_REASONS[@]} == 0)) || STUDY_VALID=no

ARMS_TEXT=""
for i in "${!ARM_LABEL[@]}"; do
    ARMS_TEXT="$ARMS_TEXT${ARMS_TEXT:+ }${ARM_LABEL[$i]}:${ARM_COMMIT[$i]}:${ARM_SHA[$i]}"
done
ARMS_TEXT="$ARMS_TEXT h3aa:copy-of-h3:$AA_SHA"

TABLE_ARGS=(--short-run "$SHORT_RUN_S" --slack "$ENTRY_SLACK"
    --band-percentile "$BAND_PERCENTILE" --physical-cores "$PHYSICAL_CORES"
    "$ENTRY_META" "$TOTALS" "$SCALING_META" "$ARMS_TEXT")

{
    echo "<!-- Generated by benchmarks/north_star.sh. Do not edit; rerun the script. -->"
    echo
    echo "# North-star engine study, registered"
    echo
    echo "The public performance statements of this release come from this record"
    echo "and from nothing else."
    echo
    if ((DRAFT != 0 || TOPOLOGY_VOID != 0)); then
        echo "**VOID.** $(if ((DRAFT != 0)); then echo "The corpus is an unfrozen draft."; fi) $(if ((TOPOLOGY_VOID != 0)); then echo "$TOPOLOGY_PROBLEM."; fi)"
        echo
    fi
    if [[ "$NS_ONLY" != "*" ]]; then
        echo "**Filtered.** \`NS_ONLY=$NS_ONLY\` ran part of the corpus, so this run grades nothing."
        echo
    fi
    if [[ "$STUDY_VALID" != yes ]]; then
        echo "**Not a complete study.** The corpus validity rule is not met:"
        for reason in "${INVALID_REASONS[@]}"; do
            echo "- $reason"
        done
        echo
    fi
    emit_provenance_md
    for i in "${!ARM_LABEL[@]}"; do
        echo "- arm \`${ARM_LABEL[$i]}\`: commit \`${ARM_COMMIT[$i]}\` sha256 \`${ARM_SHA[$i]}\`, version ${ARM_VERSION[$i]}"
    done
    echo "- arm \`h3aa\`: the A/A control, a copy of \`h3\`, sha256 \`$AA_SHA\`"
    for name in ripser ripser-coeff ripser-f64 ripser-coeff-f64; do
        if [[ -n "${COMPETITOR_BIN[$name]:-}" ]]; then
            echo "- competitor \`$name\`: \`$(basename "${COMPETITOR_BIN[$name]}")\` sha256 \`$(sha256 "${COMPETITOR_BIN[$name]}")\`"
        else
            echo "- competitor \`$name\`: not given"
        fi
    done
    echo "- ripser build: \`$RIPSER_FLAGS\`"
    echo "- giotto-ph: $GPH_VERSION (registered $GPH_WANT_VERSION)"
    echo "- corpus: \`$(basename "$CORPUS")\` version $CORPUS_VERSION dated $CORPUS_DATE, sha256 \`$CORPUS_SHA_AT_TIMING\`"
    echo "- pinning: 1 core \`$CORES_1\`, 2 cores \`$CORES_2\`, $PHYSICAL_CORES cores \`$CORES_4\`; allowed set \`$ALLOWED\`"
    echo "- controller: the runner, \`measure.py\`, and its peak RSS sampler on CPU \`$CONTROLLER_CPU\`; $CONTROLLER_CHECK"
    echo "- topology check: $TOPOLOGY_CHECK"
    echo "- routing rule at h3: \`$ROUTING_RULE\`"
    echo "- required competitors: \`$REQUIRED_COMPETITORS\`"
    echo "- kernel: $KERNEL"
    echo "- timing: one fresh process per run for holos and ripser, timed by \`measure.py\`, the child pinned by \`MEASURE_AFFINITY\`; 1 warm-up, which is the agreement run, plus at least $REPS timed repetitions per command, rounded up per entry to a multiple of its command count and rotated"
    echo "- giotto-ph is timed in process around \`ripser_parallel\` alone, so its number excludes process start and input parse, which holos pays inside its own. The comparison favors giotto-ph."
    echo "- agreement: every arm and every competitor against the \`h3\` diagram, as interval multisets within $TOLERANCE"
    echo "- entry filter: \`$NS_ONLY\`"
    echo
    python3 "$HERE/north_star_tables.py" "${TABLE_ARGS[@]}"
} >"$RESULTS_MD"

{
    echo "SUMMARY entries=$(wc -l <"$DONE_IDS") arms=$ARMS_TEXT"
    python3 "$HERE/north_star_tables.py" --text "${TABLE_ARGS[@]}"
} >>"$RESULTS"

{
    echo "# Written by benchmarks/north_star.sh at the end of a run. It names what"
    echo "# produced the record beside it. Do not edit."
    echo "corpus_file=$(basename "$CORPUS")"
    echo "corpus_version=$CORPUS_VERSION"
    echo "corpus_date=$CORPUS_DATE"
    echo "corpus_sha256=$CORPUS_SHA_AT_TIMING"
    echo "run_date=$PROV_DATE"
    echo "holos_commit=$PROV_COMMIT"
    for i in "${!ARM_LABEL[@]}"; do
        echo "arm=${ARM_LABEL[$i]} commit=${ARM_COMMIT[$i]} sha256=${ARM_SHA[$i]} version=${ARM_VERSION[$i]}"
    done
    echo "arm=h3aa commit=copy-of-h3 sha256=$AA_SHA version=$("$AA_BIN" --version)"
    for name in ripser ripser-coeff ripser-f64 ripser-coeff-f64; do
        if [[ -n "${COMPETITOR_BIN[$name]:-}" ]]; then
            echo "competitor=$name file=$(basename "${COMPETITOR_BIN[$name]}") sha256=$(sha256 "${COMPETITOR_BIN[$name]}")"
        else
            echo "competitor=$name file=none"
        fi
    done
    echo "ripser_build=$RIPSER_FLAGS"
    echo "gph_version=$GPH_VERSION"
    echo "gph_registered_version=$GPH_WANT_VERSION"
    echo "cpus_allowed=$ALLOWED"
    echo "cores_1=$CORES_1"
    echo "cores_2=$CORES_2"
    echo "cores_$PHYSICAL_CORES=$CORES_4"
    echo "controller_cpu=$CONTROLLER_CPU"
    echo "controller_check=$CONTROLLER_CHECK"
    echo "topology_check=$TOPOLOGY_CHECK"
    echo "routing_rule=$ROUTING_RULE"
    echo "cpu=$PROV_CPU"
    echo "cpu_topology=$PROV_TOPOLOGY"
    echo "kernel=$KERNEL"
    echo "rustc=$PROV_RUSTC_VERSION"
    echo "cargo=$PROV_CARGO_VERSION"
    echo "reps_minimum=$REPS"
    echo "tolerance=$TOLERANCE"
    echo "filter=$NS_ONLY"
    echo "topology_verified=$( ((TOPOLOGY_VOID == 0)) && echo yes || echo no)"
    echo "draft_corpus=$( ((DRAFT == 0)) && echo no || echo yes)"
    echo "voided=$( ((ANY_VOID == 0)) && echo no || echo yes)"
    echo "clean_tree=$CLEAN_TREE"
    echo "arms_frozen=$ARMS_FROZEN"
    echo "required_competitors=$REQUIRED_COMPETITORS"
    echo "missing_competitors=${MISSING_COMPETITORS:-none}"
    echo "study_valid=$STUDY_VALID"
    for reason in "${INVALID_REASONS[@]+"${INVALID_REASONS[@]}"}"; do
        echo "study_invalid_reason=$reason"
    done
    cat "$DONE_IDS"
} >"$MANIFEST"

echo "Results written to $RESULTS, $RESULTS_MD, and $MANIFEST." >&2
echo "Do not copy numbers into documents by hand; rerun this script instead." >&2

if ((ANY_VOID != 0)); then
    echo "FAILURE: at least one entry lost its agreement pass or an arm exited" >&2
    echo "         nonzero. The timings of that entry do not count." >&2
    exit 1
fi
