#!/usr/bin/env bash
# Scaling study for the ordered speculative edge collapse. The corpus is
# benchmarks/collapse_corpus_v06.toml, and its [meta] block quotes the
# decision rule. Neither changes in response to a result. The rounds study
# has its own runner, collapse_scaling_rounds.sh, and its own corpus.
#
# Every entry gets one seeded cloud and one threshold T = tau * enclosing
# radius. crates/collapse-bench then runs every configuration of that entry in
# one process and times each phase on its own clock. For an entry with thread
# counts {c1, ..., cT} and P = cT, the configurations are:
#   v1-c1   version 1 serial collapse, the pipeline baseline
#   v1o-cN  ordered execution of the version 1 schedule, N collapse workers,
#           one configuration per thread count, clocked phase by phase
#   v1p-cP  the pipeline with the ordered schedule selected, at P workers,
#           clocked end to end: one run-wide pool drives the ordered
#           collapse and the reduction
#   v2-cN   version 2 collapse, N collapse workers, historical context
#   none    no collapse
# That is 2T + 3 configurations. Their diagrams must all be identical before
# any timing counts. The ordered configurations run the version 1 schedule, so
# they face a second gate: the collapsed matrix and the whole certificate must
# equal v1-c1's, floats by bits. The driver checks both itself and exits
# nonzero on a mismatch.
#
# The end-to-end ratio compares v1-c1 against v1p-cP. The v1o phase clocks are
# diagnostics: that path builds a pool for the collapse and lets the reduction
# build another, which is not what the library ships.
#
# The timed repetitions are counterbalanced inside the driver. Each repetition
# starts the configuration cycle one place further on, so a thermal ramp
# cannot settle on one configuration.
#
# Peak RSS needs one process per pipeline, because the driver's own VmHWM
# covers every configuration before it. Three extra single-repetition runs,
# --mode v1, --mode v1p and --mode v2, supply the isolated figures. The first
# two are decision-rule inputs; the third is context.
#
# Protocol: run the screen first, then --confirm for the held-out set. A
# screen run that voids no entry writes results_ordered_manifest.txt, which
# records the corpus version and sha256, the entry filter, the commit, and
# every screen entry it finished. --confirm refuses to start unless that
# manifest covers the whole registered screen, ran unfiltered, ran at this
# checkout's commit, and the corpus still hashes the same.
#
# Manifest lifecycle: a screen run removes the manifest before it writes the
# first byte of its results, and writes a fresh one only after it finishes
# with no voided entry. An interrupted screen therefore leaves partial results
# and no manifest, and --confirm stays locked. This machine reboots under
# load, so the crash window is real.
#
# Registration gate: registered results need a known CPU topology, and the
# frozen P of every headline entry must equal the number of physical cores in
# the allowed CPU set. The runner refuses otherwise.
#
# Results: benchmarks/results_ordered_screen.txt (log) and .md (tables), both
# truncated on every rerun. --confirm writes results_ordered_confirm.{txt,md}
# instead, so a confirmation run cannot overwrite the screen it depends on.
# Both are gitignored.
#
# CARGO may carry a toolchain: CARGO="cargo +1.92" ./collapse_scaling_ordered.sh
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: collapse_scaling_ordered.sh [--confirm] [-h]

Run the ordered-collapse scaling corpus. Without --confirm the script runs the
[[screen]] entries. With --confirm it runs the held-out [[confirm]] entries,
which needs a complete screen run of this same corpus first.

Environment:
  CARGO           cargo invocation (may carry a toolchain), default "cargo"
  CORPUS          corpus file, default benchmarks/collapse_corpus_v06.toml
  REPS            minimum timed repetitions per configuration, default 5,
                  the preregistered minimum. Each entry runs the smallest
                  multiple of its configuration count at or above REPS, so
                  the rotation stays balanced. That count is 2T + 3 for T
                  collapse thread counts: 7 on the headline grid, 11 on the
                  strong-scaling probes. The record states the count per
                  entry, and an unbalanced entry is void
  ONLY            glob over entry ids, default all. A filtered screen run
                  cannot unlock --confirm: the manifest records the filter,
                  and --confirm refuses a filter
  ALLOW_DIRTY     set to 1 to benchmark a dirty worktree (recorded as -DIRTY)
  ALLOW_DRAFT     set to 1 to run a version 0 corpus; the results then carry
                  a DRAFT-CORPUS marker and are void
  ALLOW_NO_SCREEN set to 1 to run --confirm without a verified screen
                  manifest; the results then carry a SCREEN-PROTOCOL-BYPASSED
                  marker and are void for the decision rule
  ALLOW_ANY_TOPOLOGY
                  set to 1 to run with an unknown CPU topology, or with a
                  frozen P that does not match the physical cores of the
                  allowed set; the results then carry a TOPOLOGY-UNVERIFIED
                  marker and are void

The driver reports cost_weighted_invalidated_fraction as the median repair
wall time of the ordered run over the median collapse phase wall time of the
serial version 1 run (v1-c1), both medians over the timed repetitions. The
registered predictor names the serial version 1 predicate wall time as the
denominator. The serial path has no separate predicate clock, so this study
uses the whole serial v1 collapse phase, which is measurable and never smaller
than that predicate time. The reported fraction is therefore a lower bound on
the registered one.
EOF
}

SET=screen
for arg in "$@"; do
    case "$arg" in
        --confirm) SET=confirm ;;
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

CORPUS="${CORPUS:-$HERE/collapse_corpus_v06.toml}"
REPS="${REPS:-5}"
ONLY="${ONLY:-*}"

if ! [[ "$REPS" =~ ^[0-9]+$ ]] || ((REPS < 5)); then
    echo "error: REPS=$REPS is not an integer at or above the preregistered minimum of 5" >&2
    exit 1
fi
if [[ "$SET" == confirm && "$ONLY" != "*" ]]; then
    echo "error: --confirm takes the whole confirmation set; ONLY=$ONLY filters it" >&2
    echo "       A filtered confirmation is not the registered comparison." >&2
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

SCREEN_RESULTS="$HERE/results_ordered_screen.txt"
MANIFEST="$HERE/results_ordered_manifest.txt"
RESULTS="$SCREEN_RESULTS"
RESULTS_MD="$HERE/results_ordered_screen.md"
if [[ "$SET" == confirm ]]; then
    RESULTS="$HERE/results_ordered_confirm.txt"
    RESULTS_MD="$HERE/results_ordered_confirm.md"
fi

# corpus_meta : "version=<v> date=<d>" from [meta].
corpus_meta() {
    python3 - "$CORPUS" <<'EOF'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    meta = tomllib.load(f).get("meta", {})
version = meta.get("version")
date = meta.get("date")
if not isinstance(version, int) or version < 0 or not date:
    sys.exit("corpus [meta] needs an integer version and a date")
print(f"version={version} date={date}")
EOF
}

# corpus_entries TABLE : one tab-separated row per entry, fields in the order
# the loop below reads them. Thread counts fall back to [meta.grid]. Fails on
# a missing field or a duplicate id, so a corpus edit cannot silently drop an
# axis.
corpus_entries() {
    python3 - "$CORPUS" "$1" <<'EOF'
import sys
import tomllib

path, table = sys.argv[1], sys.argv[2]
with open(path, "rb") as f:
    corpus = tomllib.load(f)
grid = corpus.get("meta", {}).get("grid", {})
entries = corpus.get(table, [])
if not entries:
    sys.exit(f"corpus has no [[{table}]] entries")
fields = ("id", "family", "n", "coord_dim", "max_dim", "tau", "modulus", "seed", "headline")
seen = set()
for entry in entries:
    missing = [k for k in fields if k not in entry]
    if missing:
        sys.exit(f"entry {entry.get('id', '?')} lacks: {', '.join(missing)}")
    if entry["id"] in seen:
        sys.exit(f"duplicate entry id {entry['id']}")
    seen.add(entry["id"])
    collapse_threads = entry.get("collapse_threads", grid.get("collapse_threads"))
    reducer_threads = entry.get("reducer_threads", grid.get("reducer_threads"))
    if not collapse_threads or not reducer_threads:
        sys.exit(f"entry {entry['id']} has no thread counts and [meta.grid] has no default")
    row = [str(entry[k]) for k in fields[:-1]]
    row.append("yes" if entry["headline"] else "no")
    row.append(",".join(str(t) for t in collapse_threads))
    row.append(str(reducer_threads))
    row.append(str(entry.get("chain", "none")))
    row.append(str(entry.get("chain_index", 0)))
    print("\t".join(row))
EOF
}

# corpus_ids TABLE : one entry id per line, in file order.
corpus_ids() {
    python3 - "$CORPUS" "$1" <<'EOF'
import sys
import tomllib

path, table = sys.argv[1], sys.argv[2]
with open(path, "rb") as f:
    corpus = tomllib.load(f)
for entry in corpus.get(table, []):
    print(entry["id"])
EOF
}

# allowed_cpus : the logical CPUs this process may run on, or "unknown".
allowed_cpus() {
    (grep -m1 '^Cpus_allowed_list' /proc/self/status | cut -f2) 2>/dev/null || echo unknown
}

# physical_cores LIST : distinct physical cores behind the logical CPUs in
# LIST, or "unknown" when nothing maps them. SMT siblings share a core, so
# eight logical CPUs can be four cores. _common.sh records the same map in the
# provenance block; this reads it as a count.
physical_cores() {
    local cores
    cores="$(lscpu -p=CPU,CORE,SOCKET 2>/dev/null | awk -F, -v allowed="$1" '
        BEGIN {
            if (allowed == "" || allowed == "unknown") exit 1
            n = split(allowed, parts, ",")
            for (i = 1; i <= n; i++) {
                if (split(parts[i], r, "-") == 2) { for (c = r[1]; c <= r[2]; c++) ok[c] = 1 }
                else { ok[parts[i]] = 1 }
            }
        }
        # Core ids repeat across sockets, so identity is the pair.
        /^[0-9]/ { if ($1 in ok) core[$3 "," $2] = 1 }
        END { n = 0; for (c in core) n++; if (n == 0) exit 1; print n }
    ')" || cores=""
    echo "${cores:-unknown}"
}

# require_screen_manifest : the confirmation set is held out. It may run only
# after a complete screen run of this exact corpus. Running it earlier, or
# against an edited corpus, turns the study into one big grid and voids the
# preregistration.
require_screen_manifest() {
    local hint="       rerun the screen over the whole corpus, or set ALLOW_NO_SCREEN=1 to void this run"
    if [[ ! -s "$SCREEN_RESULTS" ]]; then
        echo "error: no screen results; $(basename "$SCREEN_RESULTS") is missing or empty" >&2
        echo "$hint" >&2
        exit 1
    fi
    if [[ ! -s "$MANIFEST" ]]; then
        echo "error: no screen manifest at $(basename "$MANIFEST"); the screen never finished" >&2
        echo "$hint" >&2
        exit 1
    fi

    local want_sha got_sha got_version got_filter
    want_sha="$(sha256 "$CORPUS")"
    got_sha="$(awk -F= '$1 == "corpus_sha256" { print $2; exit }' "$MANIFEST")"
    got_version="$(awk -F= '$1 == "corpus_version" { print $2; exit }' "$MANIFEST")"
    if [[ "$got_sha" != "$want_sha" ]]; then
        echo "error: $(basename "$CORPUS") changed after the screen ran" >&2
        echo "       screened: version $got_version sha256 $got_sha" >&2
        echo "       on disk:  version $CORPUS_VERSION sha256 $want_sha" >&2
        echo "       restore the corpus the screen used, or screen the new one" >&2
        exit 1
    fi

    # A screen run under ONLY=... covers part of the corpus. Its entry list
    # can still name every registered id when the glob is wide, so the filter
    # itself is checked and not inferred from the list.
    got_filter="$(awk -F= '$1 == "filter" { print $2; exit }' "$MANIFEST")"
    if [[ "$got_filter" != "*" ]]; then
        echo "error: the screen ran filtered (filter=${got_filter:-<absent>}), so it screened part of the corpus" >&2
        echo "$hint" >&2
        exit 1
    fi

    # The screen and its confirmation must measure one build. The manifest
    # records the commit; here it is checked against this checkout.
    local want_commit got_commit
    want_commit="$(head_commit)"
    got_commit="$(awk -F= '$1 == "holos_commit" { print $2; exit }' "$MANIFEST")"
    if [[ "$want_commit" == unknown ]]; then
        echo "error: this checkout reports no commit, so it cannot be matched against the screen" >&2
        echo "       screened: ${got_commit:-<absent>}" >&2
        echo "$hint" >&2
        exit 1
    fi
    if [[ "$got_commit" != "$want_commit" ]]; then
        echo "error: the screen ran at another commit than this checkout" >&2
        echo "       screened: ${got_commit:-<absent>}" >&2
        echo "       on disk:  $want_commit" >&2
        echo "       check out the commit the screen ran at, or screen this one" >&2
        exit 1
    fi
    if [[ "$want_commit" == *-DIRTY ]]; then
        echo "warning: screen and confirmation both run a dirty worktree." >&2
        echo "         The -DIRTY marker names no tree, so the commit match is weak." >&2
    fi

    local got_topology
    got_topology="$(awk -F= '$1 == "topology_verified" { print $2; exit }' "$MANIFEST")"
    if [[ "$got_topology" != "yes" ]]; then
        echo "error: the screen manifest does not record a verified topology" >&2
        echo "       recorded: ${got_topology:-<absent>}" >&2
        echo "       a screen that ran with ALLOW_ANY_TOPOLOGY cannot unlock confirmation" >&2
        exit 1
    fi

    local missing=() id ids
    ids="$(corpus_ids screen)"
    if [[ -z "$ids" ]]; then
        echo "error: the corpus registers no [[screen]] entries to check against" >&2
        exit 1
    fi
    while read -r id; do
        if ! grep -qxF "entry=$id" "$MANIFEST"; then
            missing+=("$id")
        fi
    done <<<"$ids"
    if ((${#missing[@]} > 0)); then
        echo "error: the screen manifest lacks ${#missing[@]} registered screen entries:" >&2
        printf '       %s\n' "${missing[@]}" >&2
        echo "$hint" >&2
        exit 1
    fi
}

# build_driver : release build of the in-process driver. It sets the same
# display strings emit_provenance reads, so the recorded identity is the
# binary that produced the timings.
build_driver() {
    $CARGO build --release -p collapse-bench --manifest-path "$ROOT/Cargo.toml" >&2
    HOLOS_BIN="$ROOT/target/release/collapse-bench"
    HOLOS_BIN_DISPLAY="target/release/collapse-bench"
    BUILD_CMD_DISPLAY="$CARGO build --release -p collapse-bench"
}

# row FILE PATTERN... : the first line of FILE holding every given k=v field.
row() {
    local file="$1"
    shift
    awk -v pats="$*" '
        BEGIN { n = split(pats, p, " ") }
        {
            ok = 1
            for (i = 1; i <= n; i++) {
                found = 0
                for (j = 1; j <= NF; j++) {
                    if ($j == p[i]) { found = 1; break }
                }
                if (!found) { ok = 0; break }
            }
            if (ok) { print; exit }
        }
    ' "$file"
}

# ratio A B -> A / B, "n/a" when either is missing or zero.
ratio() {
    awk -v a="${1:-}" -v b="${2:-}" 'BEGIN {
        if (a + 0 == 0 || b + 0 == 0) print "n/a"; else printf "%.2f", a / b
    }'
}

# delta A B -> A - B as a signed integer, "n/a" when either is missing.
delta() {
    awk -v a="${1:-}" -v b="${2:-}" 'BEGIN {
        if (a == "" || b == "") print "n/a"; else printf "%+d", a - b
    }'
}

or_na() {
    if [[ -n "${1:-}" ]]; then echo "$1"; else echo "n/a"; fi
}

# rss_mb KB : peak RSS in MB, "n/a" when the probe left nothing. A missing
# figure reads as missing rather than as zero.
rss_mb() {
    if [[ -n "${1:-}" ]]; then kb_to_mb "$1"; else echo "n/a"; fi
}

# rss_kb STATS : the peak RSS of one probe in kB, empty when the probe
# measured nothing. measure.py prints 0 when it never caught the child after
# exec, so 0 is a miss and never a reading.
rss_kb() {
    local kb
    kb="$(field max_rss_kb "${1:-}")"
    if [[ -n "$kb" && "$kb" != 0 ]]; then
        echo "$kb"
    fi
}

# void_cells N : N table cells reading void, with the leading pipes. A void
# row must hold as many cells as its header, or the table breaks.
void_cells() {
    local i out=""
    for ((i = 0; i < $1; i++)); do
        out="$out| void "
    done
    printf '%s' "$out"
}

# void_entry_rows ID HEADLINE FAMILY N COORD_DIM MAX_DIM TAU MODULUS : one
# void row in each of the four tables. A voided entry keeps its place, so a
# reader sees the hole instead of a shorter table.
void_entry_rows() {
    echo "| $1 | $2 | $3 | $4 | $5 | $6 | $7 | $8 $(void_cells 11)| VOID |" >>"$CFG_ROWS"
    echo "| $1 | $2 $(void_cells 21)|" >>"$TIME_ROWS"
    echo "| $1 | $2 $(void_cells 17)|" >>"$ORD_ROWS"
    echo "| $1 $(void_cells 17)|" >>"$V2_ROWS"
}

META="$(corpus_meta)"
CORPUS_VERSION="$(field version "$META")"
CORPUS_DATE="$(field date "$META")"

DRAFT=0
if [[ "$CORPUS_VERSION" == "0" ]]; then
    if [[ "${ALLOW_DRAFT:-}" == "1" ]]; then
        DRAFT=1
        echo "warning: ALLOW_DRAFT=1 runs a version 0 corpus." >&2
        echo "         That corpus is not frozen, so this run is void and says so." >&2
    else
        echo "error: $(basename "$CORPUS") is version 0 and not frozen" >&2
        echo "       Freeze it, or set ALLOW_DRAFT=1 to make a void exploratory run." >&2
        exit 1
    fi
fi

BYPASS=0
if [[ "$SET" == confirm ]]; then
    if [[ "${ALLOW_NO_SCREEN:-}" == "1" ]]; then
        BYPASS=1
        echo "warning: ALLOW_NO_SCREEN=1 skips the screen manifest check." >&2
        echo "         This run is void for the decision rule, and says so in its header." >&2
    else
        require_screen_manifest
    fi
fi

mkdir -p "$DATA"
CFG_ROWS="$DATA/scaling_rows_cfg_$SET.md"
TIME_ROWS="$DATA/scaling_rows_time_$SET.md"
ORD_ROWS="$DATA/scaling_rows_ordered_$SET.md"
V2_ROWS="$DATA/scaling_rows_v2_$SET.md"
ENTRY_FILE="$DATA/scaling_entries_$SET.tsv"
DONE_IDS="$DATA/scaling_done_$SET.txt"
: >"$CFG_ROWS"
: >"$TIME_ROWS"
: >"$ORD_ROWS"
: >"$V2_ROWS"
: >"$DONE_IDS"
corpus_entries "$SET" >"$ENTRY_FILE"

# Topology gate. The corpus p_rule reads P from the topology provenance, so a
# run whose topology is unknown cannot show that the headline arm ran one
# worker per physical core. The check covers the entries this run executes,
# and it runs before anything is measured or written.
CPUS_ALLOWED="$(allowed_cpus)"
PHYSICAL_CORES="$(physical_cores "$CPUS_ALLOWED")"
TOPOLOGY_PROBLEM=""
MISMATCHED=()
if [[ "$CPUS_ALLOWED" == unknown || "$PHYSICAL_CORES" == unknown ]]; then
    TOPOLOGY_PROBLEM="the topology of the allowed CPU set is unknown (cpus allowed: $CPUS_ALLOWED)"
else
    while IFS=$'\t' read -r id _family _n _coord_dim _max_dim _tau _modulus _seed headline collapse_threads reducer_threads _chain _chain_index; do
        if [[ "$id" != $ONLY || "$headline" != yes ]]; then
            continue
        fi
        p="${collapse_threads##*,}"
        if [[ "$p" != "$PHYSICAL_CORES" || "$reducer_threads" != "$PHYSICAL_CORES" ]]; then
            MISMATCHED+=("$id (P=$p reducer=$reducer_threads)")
        fi
    done <"$ENTRY_FILE"
    if ((${#MISMATCHED[@]} > 0)); then
        TOPOLOGY_PROBLEM="the allowed CPU set has $PHYSICAL_CORES physical cores, and headline entries of this run take another P"
    fi
fi

TOPOLOGY_VOID=0
if [[ -n "$TOPOLOGY_PROBLEM" ]]; then
    if [[ "${ALLOW_ANY_TOPOLOGY:-}" == "1" ]]; then
        TOPOLOGY_VOID=1
        echo "warning: ALLOW_ANY_TOPOLOGY=1 runs without a verified topology." >&2
        echo "         $TOPOLOGY_PROBLEM" >&2
        echo "         This run is void, and says so in its header." >&2
    else
        echo "error: $TOPOLOGY_PROBLEM" >&2
        if ((${#MISMATCHED[@]} > 0)); then
            printf '       %s\n' "${MISMATCHED[@]}" >&2
        fi
        echo "       The corpus p_rule sets P to the physical cores of the pinned CPU set." >&2
        echo "       Pin the run to a set of P physical cores (taskset), or freeze the" >&2
        echo "       corpus at the P this machine has, or set ALLOW_ANY_TOPOLOGY=1 to make" >&2
        echo "       a void exploratory run." >&2
        exit 1
    fi
fi

TOPOLOGY_NOTE="topology: cpus allowed $CPUS_ALLOWED, $PHYSICAL_CORES physical cores; every headline entry of this run takes P = $PHYSICAL_CORES collapse and reducer workers"
if ((TOPOLOGY_VOID != 0)); then
    TOPOLOGY_NOTE="topology: UNVERIFIED, $TOPOLOGY_PROBLEM"
fi

HEADER="holos ordered-collapse scaling run ($SET set)"
if ((BYPASS != 0)); then
    HEADER="SCREEN-PROTOCOL-BYPASSED: no verified screen manifest backs this run.
Its numbers are void for the decision rule.
$HEADER"
fi
if ((TOPOLOGY_VOID != 0)); then
    HEADER="TOPOLOGY-UNVERIFIED: $TOPOLOGY_PROBLEM.
Its numbers are void, and no claim may cite them.
$HEADER"
fi
if ((DRAFT != 0)); then
    HEADER="DRAFT-CORPUS: $(basename "$CORPUS") is version 0 and not frozen.
Its numbers are void, and no claim may cite them.
$HEADER"
fi

build_driver

# Remove the old manifest before the first byte of results is written. A crash
# between here and the end of the run then leaves partial results and no
# manifest, instead of a complete manifest that vouches for them.
if [[ "$SET" == screen ]]; then
    rm -f "$MANIFEST"
fi
# The tables are written once, at the end. Remove the old ones now, so a
# crash cannot leave a complete-looking table from an earlier run beside a
# partial log.
rm -f "$RESULTS_MD"
emit_provenance "$RESULTS" "$HEADER"

{
    echo "corpus: $(basename "$CORPUS") version $CORPUS_VERSION dated $CORPUS_DATE"
    echo "$TOPOLOGY_NOTE"
    echo "set: $SET  entries filtered by: $ONLY"
    echo "driver: crates/collapse-bench, one process per entry, one clock per phase"
    echo "repeats: 1 agreement run + at least $REPS timed repetitions per configuration, rounded up per entry to a multiple of its configuration count; median and IQR"
    echo "configurations per entry: v2-cN and v1o-cN for each of the entry's collapse thread counts, plus v1-c1, v1p-cP, and none; P = the entry's last collapse thread count"
    echo "end to end: v1-c1 against v1p-cP, the one-pool pipeline with the ordered schedule; the v1o phase clocks are diagnostics"
    echo "repetition order: counterbalanced in the driver, one cyclic rotation per repetition; see its kind=order lines"
    echo "diagram comparison: exact, bar for bar, inside the driver; a mismatch voids the entry"
    echo "ordered gate: v1o matrix and certificate against v1-c1, floats by bits, inside the driver"
    echo "peak RSS: one extra single-repetition process per pipeline, v1, v1p at P, and v2 at P"
    echo
} >>"$RESULTS"

ANY_MISMATCH=0
ENTRIES=0

while IFS=$'\t' read -r id family n coord_dim max_dim tau modulus seed headline collapse_threads reducer_threads chain chain_index; do
    if [[ "$id" != $ONLY ]]; then
        continue
    fi
    ENTRIES=$((ENTRIES + 1))
    p="${collapse_threads##*,}"

    cloud="$DATA/scaling_${SET}_${id}.csv"
    # The driver reads the cloud and builds its own graph, so the triplet
    # file is scratch: densify_to_sparse.py needs an output path, and only
    # its metadata line is used here. One path serves every entry.
    graph_scratch="$DATA/scaling_${SET}_graph.sparse"
    python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
    graph="$(python3 "$HERE/densify_to_sparse.py" "$cloud" "$tau" "$graph_scratch")"

    threshold="$(field threshold "$graph")"
    edges_in="$(field edges "$graph")"
    density="$(field density "$graph")"
    mean_degree="$(field mean_degree "$graph")"
    max_degree="$(field max_degree "$graph")"

    base=("$HOLOS_BIN" --entry "$id" --input "$cloud" --threshold "$threshold"
        --max-dim "$max_dim" --modulus "$modulus" --reducer-threads "$reducer_threads")
    # 2T + 3 configurations; the repetition count is the smallest multiple
    # of that at or above REPS, so every configuration takes every position
    # of the rotation equally often.
    configs_n=$(( 2 * $(tr ',' '\n' <<<"$collapse_threads" | grep -c .) + 3 ))
    reps=$(( (REPS + configs_n - 1) / configs_n * configs_n ))
    main_cmd=("${base[@]}" --collapse-threads "$collapse_threads" --reps "$reps" --mode all)
    probe_v1=("${base[@]}" --collapse-threads 1 --reps 1 --mode v1)
    probe_v1p=("${base[@]}" --collapse-threads "$p" --reps 1 --mode v1p)
    probe_v2=("${base[@]}" --collapse-threads "$p" --reps 1 --mode v2)

    rows="$DATA/scaling_${SET}_${id}.rows"
    errf="$DATA/scaling_${SET}_${id}.err"
    probe1_out="$DATA/scaling_${SET}_${id}_rss_v1.rows"
    probe1p_out="$DATA/scaling_${SET}_${id}_rss_v1p.rows"
    probe2_out="$DATA/scaling_${SET}_${id}_rss_v2.rows"

    {
        echo "== id=$id family=$family n=$n coord_dim=$coord_dim max_dim=$max_dim tau=$tau modulus=$modulus seed=$seed"
        echo "headline=$headline chain=$chain chain_index=$chain_index collapse_threads=$collapse_threads reducer_threads=$reducer_threads"
        echo "graph: $graph"
        # Recorded commands stay repo-relative, as the provenance rule in
        # _common.sh demands.
        echo "driver cmd: ${main_cmd[*]//$ROOT\//}"
    } >>"$RESULTS"

    if ! run_stats="$(measure_err "$rows" "$errf" "${main_cmd[@]}")"; then
        ANY_MISMATCH=1
        {
            echo "VOID: the driver exited nonzero; no timing is recorded for this entry"
            sed 's/^/  /' "$errf"
            echo
        } >>"$RESULTS"
        void_entry_rows "$id" "$headline" "$family" "$n" "$coord_dim" "$max_dim" "$tau" "$modulus"
        echo "id=$id VOID (driver exited nonzero; see $(basename "$errf"))" >&2
        continue
    fi

    rss_v1=""
    rss_v1p=""
    rss_v2=""
    if probe1_stats="$(measure "$probe1_out" "${probe_v1[@]}")"; then
        rss_v1="$(rss_kb "$probe1_stats")"
    fi
    if probe1p_stats="$(measure "$probe1p_out" "${probe_v1p[@]}")"; then
        rss_v1p="$(rss_kb "$probe1p_stats")"
    fi
    if probe2_stats="$(measure "$probe2_out" "${probe_v2[@]}")"; then
        rss_v2="$(rss_kb "$probe2_stats")"
    fi
    # Peak RSS of the serial pipeline and of the shipped pipeline is a
    # decision-rule input. A failed probe voids the entry rather than
    # certifying it with a hole; fail closed. The version 2 probe is context,
    # so its failure only leaves a hole in the context table.
    if [ -z "$rss_v1" ] || [ -z "$rss_v1p" ]; then
        ANY_MISMATCH=1
        echo "VOID: an RSS probe left no reading; the entry is missing a decision-rule input" >>"$RESULTS"
        void_entry_rows "$id" "$headline" "$family" "$n" "$coord_dim" "$max_dim" "$tau" "$modulus"
        echo "id=$id VOID (RSS probe left no reading)" >&2
        continue
    fi

    coll_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=collapse)")"
    coll_v1_iqr="$(field iqr_s "$(row "$rows" kind=phase config=v1-c1 phase=collapse)")"
    coll_o1="$(field median_s "$(row "$rows" kind=phase config=v1o-c1 phase=collapse)")"
    coll_o1_iqr="$(field iqr_s "$(row "$rows" kind=phase config=v1o-c1 phase=collapse)")"
    coll_op="$(field median_s "$(row "$rows" kind=phase "config=v1o-c$p" phase=collapse)")"
    coll_op_iqr="$(field iqr_s "$(row "$rows" kind=phase "config=v1o-c$p" phase=collapse)")"
    coll_c1="$(field median_s "$(row "$rows" kind=phase config=v2-c1 phase=collapse)")"
    coll_c1_iqr="$(field iqr_s "$(row "$rows" kind=phase config=v2-c1 phase=collapse)")"
    coll_cp="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=collapse)")"
    coll_cp_iqr="$(field iqr_s "$(row "$rows" kind=phase "config=v2-c$p" phase=collapse)")"
    red_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=reduce)")"
    red_v1o="$(field median_s "$(row "$rows" kind=phase "config=v1o-c$p" phase=reduce)")"
    red_v2="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=reduce)")"
    tot_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=total)")"
    tot_v1o="$(field median_s "$(row "$rows" kind=phase "config=v1o-c$p" phase=total)")"
    tot_v1p="$(field median_s "$(row "$rows" kind=phase "config=v1p-c$p" phase=total)")"
    tot_v2="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=total)")"
    tot_none="$(field median_s "$(row "$rows" kind=phase config=none phase=total)")"

    counters_v1="$(row "$rows" kind=counters config=v1-c1)"
    counters_v1o="$(row "$rows" kind=counters "config=v1o-c$p")"
    counters_v2="$(row "$rows" kind=counters "config=v2-c$p")"
    surv_v1="$(field output_edges "$counters_v1")"
    surv_v1o="$(field output_edges "$counters_v1o")"
    surv_v2="$(field output_edges "$counters_v2")"
    passes_v1="$(field epochs "$counters_v1")"
    passes_v1o="$(field epochs "$counters_v1o")"
    rounds_v2="$(field epochs "$counters_v2")"
    tests_v1="$(field edge_tests "$counters_v1")"
    tests_v2="$(field edge_tests "$counters_v2")"
    width_max="$(field batch_width_max "$counters_v2")"

    logical="$(field logical_tests "$counters_v1o")"
    physical="$(field edge_tests "$counters_v1o")"
    inflation="$(field work_inflation_derived "$counters_v1o")"
    invalidated="$(field invalidated_results "$counters_v1o")"
    repair_fraction="$(field repair_fraction_derived "$counters_v1o")"
    globals="$(field global_invalidations "$counters_v1o")"
    windows="$(field window_batches "$counters_v1o")"
    # Occupancy and the subphase clocks, both medians over the timed
    # repetitions inside the driver.
    slots_offered="$(field window_slots_offered "$counters_v1o")"
    members_formed="$(field window_members_formed "$counters_v1o")"
    members_reused="$(field window_members_reused "$counters_v1o")"
    occupancy="$(field window_occupancy_derived "$counters_v1o")"
    unused_capacity="$(field unused_window_capacity_derived "$counters_v1o")"
    predicate_s="$(field predicate_median_s "$counters_v1o")"
    retirement_s="$(field retirement_median_s "$counters_v1o")"
    repair_s="$(field repair_median_s "$counters_v1o")"
    cost_weighted="$(field cost_weighted_invalidated_fraction "$counters_v1o")"
    cost_denominator="$(field cost_weighted_denominator "$counters_v1o")"
    cost_denominator_s="$(field cost_weighted_denominator_s "$counters_v1o")"
    counters_stable="$(field counters_stable "$counters_v1o")"

    # The ordered gate runs inside the driver, which exits nonzero on a
    # mismatch. An entry whose gate never ran cannot back a claim either.
    gate="$(field checked "$(row "$rows" kind=ordered_gate "config=v1o-c$p")")"
    gate_status="match"
    entry_status="ok"
    if [[ "$gate" != yes ]]; then
        ANY_MISMATCH=1
        echo "VOID: the ordered gate did not run for v1o-c$p" >>"$RESULTS"
        void_entry_rows "$id" "$headline" "$family" "$n" "$coord_dim" "$max_dim" "$tau" "$modulus"
        echo "id=$id VOID (the ordered gate did not run)" >&2
        continue
    fi
    # The rotation is balanced only when REPS is a multiple of the
    # configuration count; the driver says which.
    balanced="$(field balanced "$(row "$rows" kind=entry)")"
    if [[ "$balanced" != yes ]]; then
        ANY_MISMATCH=1
        echo "VOID: the repetition order is not balanced ($reps repetitions over $(field configs "$(row "$rows" kind=entry)"))" >>"$RESULTS"
        void_entry_rows "$id" "$headline" "$family" "$n" "$coord_dim" "$max_dim" "$tau" "$modulus"
        echo "id=$id VOID (unbalanced repetition order)" >&2
        continue
    fi

    # The end-to-end ratio is the registered comparison only when the shipped
    # pipeline reduces as wide as the baseline does. One pool drives both
    # halves of it, so that holds when P equals the entry's reducer threads.
    # A headline entry that misses it measures another comparison.
    product_arm="$(field registered_arm "$(row "$rows" kind=config "config=v1p-c$p")")"
    product_arm="${product_arm:-n/a}"
    if [[ "$product_arm" != yes ]]; then
        entry_status="non-registered-arm"
    fi
    if [[ "$headline" == yes && "$product_arm" != yes ]]; then
        ANY_MISMATCH=1
        {
            if [[ "$product_arm" == "n/a" ]]; then
                echo "VOID: the driver recorded no v1p-c$p configuration, so this entry has no"
                echo "      shipped-pipeline timing"
            else
                echo "VOID: v1p-c$p reduces with $p workers and the baseline with $reducer_threads;"
                echo "      that is not the registered arm, and this entry is a headline entry"
            fi
        } >>"$RESULTS"
        void_entry_rows "$id" "$headline" "$family" "$n" "$coord_dim" "$max_dim" "$tau" "$modulus"
        echo "id=$id VOID (the product arm is not the registered arm)" >&2
        continue
    fi

    serial_over_ordered="$(ratio "$coll_v1" "$coll_op")"
    ordered_scaling="$(ratio "$coll_o1" "$coll_op")"
    end_to_end="$(ratio "$tot_v1" "$tot_v1p")"
    reduce_parity="$(ratio "$red_v1" "$red_v1o")"
    rss_product="$(ratio "$rss_v1p" "$rss_v1")"
    scaling_v2="$(ratio "$coll_c1" "$coll_cp")"
    end_to_end_v2="$(ratio "$tot_v1" "$tot_v2")"
    reduce_delta="$(ratio "$red_v1" "$red_v2")"
    surv_delta="$(delta "$surv_v2" "$surv_v1")"
    rss_delta="$(ratio "$rss_v2" "$rss_v1")"

    {
        cat "$rows"
        echo "configurations run: $(field configs "$(row "$rows" kind=entry)")"
        echo "run $run_stats"
        echo "rss probe v1 ${probe1_stats:-<failed>}"
        echo "rss probe v1p ${probe1p_stats:-<failed>}"
        echo "rss probe v2 ${probe2_stats:-<failed>}"
        echo "HEADLINE serial_over_ordered_c$p=$serial_over_ordered ordered_c1_over_c$p=$ordered_scaling end_to_end_v1_over_v1p=$end_to_end end_to_end_config=v1p-c$p product_arm=$product_arm reduce_v1_over_v1o=$reduce_parity rss_v1p_over_v1=$rss_product ordered_gate=$gate_status"
        echo "WORK logical_tests=$(or_na "$logical") physical_tests=$(or_na "$physical") work_inflation=$(or_na "$inflation") invalidated=$(or_na "$invalidated") repair_fraction=$(or_na "$repair_fraction") global_invalidations=$(or_na "$globals") window_batches=$(or_na "$windows") counters_stable=$(or_na "$counters_stable")"
        echo "OCCUPANCY window_slots_offered=$(or_na "$slots_offered") window_members_formed=$(or_na "$members_formed") window_members_reused=$(or_na "$members_reused") window_occupancy=$(or_na "$occupancy") unused_window_slots=$(or_na "$unused_capacity")"
        echo "SUBPHASES predicate_s=$(or_na "$predicate_s") retirement_s=$(or_na "$retirement_s") repair_s=$(or_na "$repair_s") cost_weighted_invalidated_fraction=$(or_na "$cost_weighted") denominator=$(or_na "$cost_denominator") denominator_s=$(or_na "$cost_denominator_s")"
        echo "CONTEXT v2 collapse_c1_over_c$p=$scaling_v2 end_to_end_v1_over_v2=$end_to_end_v2 reduce_v1_over_v2=$reduce_delta survivor_delta_v2_minus_v1=$surv_delta rss_v2_over_v1=$rss_delta"
        echo
    } >>"$RESULTS"

    echo "| $id | $headline | $family | $n | $coord_dim | $max_dim | $tau | $modulus | $collapse_threads | $reducer_threads | $density | $mean_degree | $max_degree | $edges_in | $(or_na "$surv_v1") | $(or_na "$surv_v1o") | $(or_na "$passes_v1") | $(or_na "$passes_v1o") | $gate_status | $entry_status |" >>"$CFG_ROWS"
    echo "| $id | $headline | $(or_na "$coll_v1") | $(or_na "$coll_v1_iqr") | $(or_na "$coll_o1") | $(or_na "$coll_o1_iqr") | $(or_na "$coll_op") | $(or_na "$coll_op_iqr") | $serial_over_ordered | $ordered_scaling | $(or_na "$red_v1") | $(or_na "$red_v1o") | $reduce_parity | $(or_na "$tot_v1") | $(or_na "$tot_v1p") | $end_to_end | $product_arm | $(or_na "$tot_v1o") | $(or_na "$tot_none") | $(rss_mb "$rss_v1") | $(rss_mb "$rss_v1p") | $rss_product | yes |" >>"$TIME_ROWS"
    echo "| $id | $headline | $(or_na "$logical") | $(or_na "$physical") | $(or_na "$inflation") | $(or_na "$invalidated") | $(or_na "$repair_fraction") | $(or_na "$globals") | $(or_na "$windows") | $(or_na "$slots_offered") | $(or_na "$members_formed") | $(or_na "$members_reused") | $(or_na "$occupancy") | $(or_na "$unused_capacity") | $(or_na "$passes_v1o") | $(or_na "$predicate_s") | $(or_na "$retirement_s") | $(or_na "$repair_s") | $(or_na "$cost_weighted") |" >>"$ORD_ROWS"
    echo "| $id | $(or_na "$coll_c1") | $(or_na "$coll_c1_iqr") | $(or_na "$coll_cp") | $(or_na "$coll_cp_iqr") | $scaling_v2 | $(or_na "$red_v2") | $(or_na "$tot_v2") | $end_to_end_v2 | $reduce_delta | $(or_na "$surv_v2") | $surv_delta | $(or_na "$rounds_v2") | $(or_na "$tests_v1") | $(or_na "$tests_v2") | $(or_na "$width_max") | $(rss_mb "$rss_v2") | $rss_delta |" >>"$V2_ROWS"
    echo "entry=$id" >>"$DONE_IDS"
    echo "id=$id serial/ordered=$serial_over_ordered end-to-end=$end_to_end inflation=${inflation:-?} invalidated=${invalidated:-?} rss=$rss_product gate=$gate_status arm=$product_arm" >&2
done <"$ENTRY_FILE"

if ((ENTRIES == 0)); then
    echo "error: filter ONLY=$ONLY matched no entry in the $SET set" >&2
    exit 1
fi

{
    echo "<!-- Generated by benchmarks/collapse_scaling_ordered.sh. Do not edit; rerun the script. -->"
    echo
    echo "# Ordered edge-collapse scaling, $SET set"
    echo
    if ((DRAFT != 0)); then
        echo "**DRAFT-CORPUS.** The corpus is version 0 and not frozen. These numbers"
        echo "are void, and no claim may cite them."
        echo
    fi
    if ((BYPASS != 0)); then
        echo "**SCREEN-PROTOCOL-BYPASSED.** ALLOW_NO_SCREEN=1 skipped the screen"
        echo "manifest check, so no verified screen backs this run. Its numbers are"
        echo "void for the decision rule."
        echo
    fi
    if ((TOPOLOGY_VOID != 0)); then
        echo "**TOPOLOGY-UNVERIFIED.** ALLOW_ANY_TOPOLOGY=1 skipped the topology"
        echo "check: $TOPOLOGY_PROBLEM. These numbers are void, and no claim may"
        echo "cite them."
        echo
    fi
    emit_provenance_md
    echo "- corpus: \`$(basename "$CORPUS")\` version $CORPUS_VERSION dated $CORPUS_DATE, $SET set, $ENTRIES entries"
    echo "- $TOPOLOGY_NOTE"
    echo "- driver: \`crates/collapse-bench\`, one process per entry, one clock per phase"
    echo "- repeats: 1 agreement run + at least $REPS timed repetitions per configuration, rounded up per entry to a multiple of its configuration count (the driver's kind=entry line records each count); median and IQR over the timed repetitions"
    echo "- counters and subphase clocks: medians over the timed repetitions, from the same repetitions as the phase medians; the agreement run supplies none"
    echo "- repetition order: counterbalanced, one cyclic rotation of the configurations per repetition; the driver records each order"
    echo "- threshold: T = tau * enclosing radius, identical for every configuration"
    echo "- diagrams: compared exactly, bar for bar, across every configuration of the entry before any timing"
    echo "- ordered gate: v1o matrix and certificate against v1-c1, field for field and floats by bits, inside the driver"
    echo "- end to end: v1-c1 against v1p-cP, the shipped pipeline, whose single pool drives the ordered collapse and the reduction"
    echo "- peak RSS: one extra single-repetition process per pipeline; the driver's own VmHWM covers the whole process and is in the .txt log only"
    echo "- decision rule: \`$(basename "$CORPUS")\`, [meta] decision_rule, quoted in the corpus"
    echo "- entry filter: \`$ONLY\`"
    echo
    echo "## Configuration and collapse yield"
    echo
    echo "P is the entry's last collapse thread count. Survivors are output edges."
    echo "The ordered execution runs the version 1 schedule, so its survivors and"
    echo "passes must equal v1's, and the gate column is the driver's exact check."
    echo "The status column reads non-registered-arm when the shipped pipeline at P"
    echo "does not reduce as wide as the baseline. A headline entry in that state is"
    echo "void."
    echo
    echo "| id | headline | family | n | dim | maxdim | tau | modulus | collapse threads | reducer threads | density | mean deg | max deg | edges in | survivors v1 | survivors v1o | passes v1 | passes v1o | ordered gate | status |"
    echo "|:--|:--|:--|--:|--:|--:|--:|--:|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|:--|"
    cat "$CFG_ROWS"
    echo
    echo "## Phase medians and the headline ratios"
    echo
    echo "Times are seconds, medians over the timed repetitions. Serial/ordered is"
    echo "the collapse phase of v1-c1 over v1o-cP, the headline ratio. C1/CP is the"
    echo "ordered collapse against itself at one worker. End to end is the v1"
    echo "pipeline over the shipped pipeline v1p-cP, and above 1.0 means the ordered"
    echo "collapse wins. Product arm says whether v1p-cP reduces as wide as the"
    echo "baseline, which is what makes it the registered comparison. Total v1o is"
    echo "the phase-split diagnostic: it builds one pool for the collapse and another"
    echo "for the reduction, so it enters no ratio. Both pipelines reduce the same"
    echo "matrix, so reduce v1/v1o sits at 1.0 up to noise. RSS v1p/v1 must stay at"
    echo "or below 1.5 for a product-positive grade."
    echo
    echo "| id | headline | collapse v1 | IQR | collapse v1o c1 | IQR | collapse v1o cP | IQR | serial/ordered | C1/CP | reduce v1 | reduce v1o | reduce v1/v1o | total v1 | total v1p | end to end | product arm | total v1o (split) | total no collapse | RSS v1 (MB) | RSS v1p (MB) | RSS v1p/v1 | diagrams identical |"
    echo "|:--|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|--:|--:|--:|--:|--:|:--|"
    cat "$TIME_ROWS"
    echo
    echo "## Ordered scheduler work, at P workers"
    echo
    echo "Every figure here is a median over the timed repetitions, taken inside the"
    echo "driver. Logical tests are the serial schedule's tests; physical tests are"
    echo "predicate evaluations. Work inflation is physical over logical and stays at"
    echo "or below 2.0 by the schedule's bound. Every invalidated result is repaired"
    echo "serially at its turn, so repair fraction is invalidated results per logical"
    echo "test: it is a count, not a cost. Occupancy is window members formed over"
    echo "window slots offered. Unused window capacity is the unfilled slots of the"
    echo "run, an upper bound on pass-tail loss: a stage fills short only when its"
    echo "scan reached the end of the edge list, which can happen mid-pass, because"
    echo "retiring a window can arm a position the form scan already passed. The"
    echo "figure is a run total and names no single stage. Predicate, retirement, and"
    echo "repair are the collapse subphase clocks of v1o-cP, and retirement includes"
    echo "the repairs."
    echo
    echo "The cost-weighted invalidated fraction is the registered ceiling predictor."
    echo "The driver computes it as the median repair wall time of the ordered run"
    echo "over the median collapse phase wall time of the serial version 1 run"
    echo "(v1-c1), both medians over the timed repetitions. The registered predictor"
    echo "names the serial version 1 predicate wall time as the denominator. The"
    echo "serial path has no separate predicate clock, so this study uses the whole"
    echo "serial v1 collapse phase, which is measurable and never smaller than that"
    echo "predicate time. The reported fraction is therefore a lower bound on the"
    echo "registered one. The .txt log carries the denominator it used and its"
    echo "seconds."
    echo
    echo "| id | headline | logical tests | physical tests | work inflation | invalidated | repair fraction | global invalidations | window batches | slots offered | members formed | members reused | occupancy | pass-unused window capacity (slots) | passes | predicate s | retirement s | repair s | cost-weighted invalidated fraction |"
    echo "|:--|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|"
    cat "$ORD_ROWS"
    echo
    echo "## Version 2 context"
    echo
    echo "Historical only. The version 2 schedule keeps a different edge set and its"
    echo "own certificate, so these rows enter no grade. The ceiling of the ordered"
    echo "schedule is never inferred from the version 2 batch width."
    echo
    echo "| id | collapse v2 c1 | IQR | collapse v2 cP | IQR | C1/CP | reduce v2 | total v2 | end to end v1/v2 | reduce v1/v2 | survivors v2 | survivor delta | rounds v2 | edge tests v1 | edge tests v2 | max batch | RSS v2 (MB) | RSS v2/v1 |"
    echo "|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|"
    cat "$V2_ROWS"
} >"$RESULTS_MD"

# The manifest is what --confirm checks. It names the corpus the screen
# covered, by version and hash, the commit and filter it ran under, and every
# entry that finished. The run removed any earlier manifest before it wrote
# its first result, so only a clean run that reaches this point leaves one.
# ANY void leaves none: a voided entry, or a run-level void such as an
# unverified topology. A void run must not be able to unlock confirmation
# through a manifest that says nothing about why it was void.
if [[ "$SET" == screen ]]; then
    if ((ANY_MISMATCH != 0 || TOPOLOGY_VOID != 0 || DRAFT != 0)); then
        if ((ANY_MISMATCH != 0)); then
            echo "No screen manifest: this run voided at least one entry." >&2
        fi
        if ((TOPOLOGY_VOID != 0)); then
            echo "No screen manifest: this run ran with an unverified topology." >&2
        fi
        if ((DRAFT != 0)); then
            echo "No screen manifest: this run used an unfrozen draft corpus." >&2
        fi
        echo "This run removed the earlier manifest before it wrote anything, so" >&2
        echo "--confirm stays locked." >&2
    else
        {
            echo "# Written by benchmarks/collapse_scaling_ordered.sh at the end of a clean screen run."
            echo "# --confirm reads it. Do not edit: an edit only fakes a screen."
            echo "corpus_file=$(basename "$CORPUS")"
            echo "corpus_version=$CORPUS_VERSION"
            echo "corpus_date=$CORPUS_DATE"
            echo "corpus_sha256=$(sha256 "$CORPUS")"
            echo "screen_date=$PROV_DATE"
            echo "holos_commit=$PROV_COMMIT"
            echo "filter=$ONLY"
            echo "topology_verified=yes"
            cat "$DONE_IDS"
        } >"$MANIFEST"
        echo "Screen manifest written to $MANIFEST." >&2
    fi
fi

echo "Results written to $RESULTS and $RESULTS_MD." >&2
echo "Do not copy numbers into documents by hand; rerun this script instead." >&2

if ((ANY_MISMATCH != 0)); then
    echo "FAILURE: at least one entry failed its diagram comparison, its ordered gate," >&2
    echo "         an RSS probe, or the registered-arm check. The timings of that" >&2
    echo "         entry do not count." >&2
    exit 1
fi
