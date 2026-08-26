#!/usr/bin/env bash
# Scaling study for the rounds schedule (algorithm version 2). The corpus is
# benchmarks/collapse_corpus_v05.toml, and its [meta] block quotes the
# decision rule. Neither changes in response to a result. The ordered study
# has its own runner, collapse_scaling_ordered.sh, and its own corpus.
#
# Every entry gets one seeded cloud and one threshold T = tau * enclosing
# radius. crates/collapse-bench then runs every configuration of that entry in
# one process and times each phase on its own clock. For thread counts
# {c1, ..., cT} and P = cT:
#   v2-cN   version 2 collapse, N collapse workers, one per thread count
#   v1-c1   version 1 serial collapse, the v0.4 pipeline baseline
#   none    no collapse
# That is T + 2 configurations: four on the headline grid, six on the
# strong-scaling probes. Their diagrams must all be identical before any
# timing counts. The driver checks that and exits nonzero on a mismatch.
#
# Peak RSS needs one process per pipeline, because the driver's own VmHWM
# covers every configuration before it. Two extra single-repetition runs,
# --mode v1 and --mode v2, supply the isolated figures the decision rule
# reads.
#
# Protocol: run the screen first, then --confirm for the held-out set. A
# screen run writes results_rounds_manifest.txt, which records the corpus
# version and sha256 and every screen entry it finished. --confirm refuses to
# start unless that manifest covers the whole registered screen and the corpus
# still hashes the same.
#
# Results: benchmarks/results_rounds_screen.txt (log) and .md (tables), both
# truncated on every rerun. --confirm writes results_rounds_confirm.{txt,md}
# instead, so a confirmation run cannot overwrite the screen it depends on.
# Both are gitignored.
#
# CARGO may carry a toolchain: CARGO="cargo +1.92" ./collapse_scaling_rounds.sh
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: collapse_scaling_rounds.sh [--confirm] [-h]

Run the rounds-collapse scaling corpus. Without --confirm the script runs the
[[screen]] entries. With --confirm it runs the held-out [[confirm]] entries,
which needs a complete screen run of this same corpus first.

Environment:
  CARGO           cargo invocation (may carry a toolchain), default "cargo"
  CORPUS          corpus file, default benchmarks/collapse_corpus_v05.toml
  REPS            minimum timed repetitions per configuration, default 5,
                  the preregistered minimum. Each entry runs the smallest
                  multiple of its configuration count at or above REPS, so
                  the rotation stays balanced. That count is T + 2 for T
                  collapse thread counts: 4 on the headline grid, 6 on the
                  strong-scaling probes. The record states the count per
                  entry, and an unbalanced entry is void
  ONLY            glob over entry ids, default all; --confirm refuses a
                  filter
  ALLOW_DIRTY     set to 1 to benchmark a dirty worktree (recorded as -DIRTY)
  ALLOW_DRAFT     set to 1 to run a version 0 corpus; the results then carry
                  a DRAFT-CORPUS marker and are void
  ALLOW_NO_SCREEN set to 1 to run --confirm without a verified screen
                  manifest; the results then carry a SCREEN-PROTOCOL-BYPASSED
                  marker and are void for the decision rule
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

CORPUS="${CORPUS:-$HERE/collapse_corpus_v05.toml}"
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

SCREEN_RESULTS="$HERE/results_rounds_screen.txt"
MANIFEST="$HERE/results_rounds_manifest.txt"
RESULTS="$SCREEN_RESULTS"
RESULTS_MD="$HERE/results_rounds_screen.md"
if [[ "$SET" == confirm ]]; then
    RESULTS="$HERE/results_rounds_confirm.txt"
    RESULTS_MD="$HERE/results_rounds_confirm.md"
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

    local want_sha got_sha got_version
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
    # can still name every registered id when the glob is wide, so the
    # filter itself is checked and not inferred from the list.
    local got_filter
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
        echo "       Freeze it at version 1 after the v0.4 study's results are consumed," >&2
        echo "       or set ALLOW_DRAFT=1 to make a void exploratory run." >&2
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

HEADER="holos parallel-collapse scaling run ($SET set)"
if ((BYPASS != 0)); then
    HEADER="SCREEN-PROTOCOL-BYPASSED: no verified screen manifest backs this run.
Its numbers are void for the decision rule.
$HEADER"
fi
if ((DRAFT != 0)); then
    HEADER="DRAFT-CORPUS: $(basename "$CORPUS") is version 0 and not frozen.
Its numbers are void, and no claim may cite them.
$HEADER"
fi

mkdir -p "$DATA"
build_driver
# Remove the old manifest before the first byte of results is written. A
# crash between here and the end of the run then leaves partial results and
# no manifest, instead of a complete manifest that vouches for them.
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
    echo "set: $SET  entries filtered by: $ONLY"
    echo "driver: crates/collapse-bench, one process per entry, one clock per phase"
    echo "repeats: 1 agreement run + at least $REPS timed repetitions per configuration, rounded up per entry to a multiple of its configuration count; median and IQR"
    echo "configurations: v2-cN at every collapse thread count of the entry, v1-c1, none; P = the entry's last collapse thread count"
    echo "diagram comparison: exact, bar for bar, inside the driver; a mismatch voids the entry"
    echo "peak RSS: one extra single-repetition process per pipeline, v1 and v2 at P workers"
    echo
} >>"$RESULTS"

CFG_ROWS="$DATA/scaling_rows_cfg_$SET.md"
TIME_ROWS="$DATA/scaling_rows_time_$SET.md"
ENTRY_FILE="$DATA/scaling_entries_$SET.tsv"
DONE_IDS="$DATA/scaling_done_$SET.txt"
: >"$CFG_ROWS"
: >"$TIME_ROWS"
: >"$DONE_IDS"
corpus_entries "$SET" >"$ENTRY_FILE"

ANY_MISMATCH=0
ENTRIES=0

while IFS=$'\t' read -r id family n coord_dim max_dim tau modulus seed headline collapse_threads reducer_threads chain chain_index; do
    if [[ "$id" != $ONLY ]]; then
        continue
    fi
    ENTRIES=$((ENTRIES + 1))
    p="${collapse_threads##*,}"

    cloud="$DATA/scaling_${SET}_${id}.csv"
    sparse="$DATA/scaling_${SET}_${id}.sparse"
    python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
    graph="$(python3 "$HERE/densify_to_sparse.py" "$cloud" "$tau" "$sparse")"

    threshold="$(field threshold "$graph")"
    edges_in="$(field edges "$graph")"
    density="$(field density "$graph")"
    mean_degree="$(field mean_degree "$graph")"
    max_degree="$(field max_degree "$graph")"

    base=("$HOLOS_BIN" --entry "$id" --input "$cloud" --threshold "$threshold"
        --max-dim "$max_dim" --modulus "$modulus" --reducer-threads "$reducer_threads")
    # T + 2 configurations; the repetition count is the smallest multiple of
    # that at or above REPS, so every configuration takes every position of
    # the rotation equally often.
    configs_n=$(( $(tr ',' '\n' <<<"$collapse_threads" | grep -c .) + 2 ))
    reps=$(( (REPS + configs_n - 1) / configs_n * configs_n ))
    main_cmd=("${base[@]}" --collapse-threads "$collapse_threads" --reps "$reps" --mode rounds)
    probe_v1=("${base[@]}" --collapse-threads 1 --reps 1 --mode v1)
    probe_v2=("${base[@]}" --collapse-threads "$p" --reps 1 --mode v2)

    rows="$DATA/scaling_${SET}_${id}.rows"
    errf="$DATA/scaling_${SET}_${id}.err"
    probe1_out="$DATA/scaling_${SET}_${id}_rss_v1.rows"
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
        echo "| $id | $headline | $family | $n | $coord_dim | $max_dim | $tau | $modulus | void | void | void | void | void | void | void | void | void | void | void | void | void | void | VOID |" >>"$CFG_ROWS"
        echo "| $id | $headline | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void |" >>"$TIME_ROWS"
        echo "id=$id VOID (driver exited nonzero; see $(basename "$errf"))" >&2
        continue
    fi

    rss_v1=""
    rss_v2=""
    if probe1_stats="$(measure "$probe1_out" "${probe_v1[@]}")"; then
        rss_v1="$(field max_rss_kb "$probe1_stats")"
    fi
    if probe2_stats="$(measure "$probe2_out" "${probe_v2[@]}")"; then
        rss_v2="$(field max_rss_kb "$probe2_stats")"
    fi
    # Peak RSS is a decision-rule input. A failed probe voids the entry
    # rather than certifying it with a hole; fail closed.
    if [ -z "$rss_v1" ] || [ -z "$rss_v2" ]; then
        ANY_MISMATCH=1
        echo "VOID: an RSS probe failed; the entry is missing a decision-rule input" >>"$RESULTS"
        echo "| $id | $headline | $family | $n | $coord_dim | $max_dim | $tau | $modulus | void | void | void | void | void | void | void | void | void | void | void | void | void | void | VOID |" >>"$CFG_ROWS"
        echo "| $id | $headline | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void |" >>"$TIME_ROWS"
        echo "id=$id VOID (RSS probe failed)" >&2
        continue
    fi
    # The rotation is balanced only when REPS is a multiple of the
    # configuration count; the driver says which.
    balanced="$(field balanced "$(row "$rows" kind=entry)")"
    if [[ "$balanced" != yes ]]; then
        ANY_MISMATCH=1
        echo "VOID: the repetition order is not balanced ($reps repetitions over $(field configs "$(row "$rows" kind=entry)"))" >>"$RESULTS"
        echo "| $id | $headline | $family | $n | $coord_dim | $max_dim | $tau | $modulus | void | void | void | void | void | void | void | void | void | void | void | void | void | void | VOID |" >>"$CFG_ROWS"
        echo "| $id | $headline | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void | void |" >>"$TIME_ROWS"
        echo "id=$id VOID (unbalanced repetition order)" >&2
        continue
    fi

    coll_c1="$(field median_s "$(row "$rows" kind=phase config=v2-c1 phase=collapse)")"
    coll_c1_iqr="$(field iqr_s "$(row "$rows" kind=phase config=v2-c1 phase=collapse)")"
    coll_cp="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=collapse)")"
    coll_cp_iqr="$(field iqr_s "$(row "$rows" kind=phase "config=v2-c$p" phase=collapse)")"
    coll_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=collapse)")"
    red_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=reduce)")"
    red_v2="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=reduce)")"
    tot_v1="$(field median_s "$(row "$rows" kind=phase config=v1-c1 phase=total)")"
    tot_v2="$(field median_s "$(row "$rows" kind=phase "config=v2-c$p" phase=total)")"
    tot_none="$(field median_s "$(row "$rows" kind=phase config=none phase=total)")"

    counters_v1="$(row "$rows" kind=counters config=v1-c1)"
    counters_v2="$(row "$rows" kind=counters "config=v2-c$p")"
    surv_v1="$(field output_edges "$counters_v1")"
    surv_v2="$(field output_edges "$counters_v2")"
    passes_v1="$(field epochs "$counters_v1")"
    rounds_v2="$(field epochs "$counters_v2")"
    tests_v1="$(field edge_tests "$counters_v1")"
    tests_v2="$(field edge_tests "$counters_v2")"
    width_max="$(field batch_width_max "$counters_v2")"

    scaling="$(ratio "$coll_c1" "$coll_cp")"
    end_to_end="$(ratio "$tot_v1" "$tot_v2")"
    reduce_delta="$(ratio "$red_v1" "$red_v2")"
    surv_delta="$(delta "$surv_v2" "$surv_v1")"
    rss_delta="$(ratio "$rss_v2" "$rss_v1")"

    {
        cat "$rows"
        echo "run $run_stats"
        echo "rss probe v1 ${probe1_stats:-<failed>}"
        echo "rss probe v2 ${probe2_stats:-<failed>}"
        echo "HEADLINE collapse_c1_over_c$p=$scaling end_to_end_v1_over_v2=$end_to_end reduce_v1_over_v2=$reduce_delta survivor_delta_v2_minus_v1=$surv_delta rss_v2_over_v1=$rss_delta"
        echo
    } >>"$RESULTS"

    echo "| $id | $headline | $family | $n | $coord_dim | $max_dim | $tau | $modulus | $collapse_threads | $reducer_threads | $density | $mean_degree | $max_degree | $edges_in | $(or_na "$surv_v1") | $(or_na "$surv_v2") | $surv_delta | $(or_na "$passes_v1") | $(or_na "$rounds_v2") | $(or_na "$tests_v1") | $(or_na "$tests_v2") | $(or_na "$width_max") | ok |" >>"$CFG_ROWS"
    echo "| $id | $headline | $(or_na "$coll_c1") | $(or_na "$coll_c1_iqr") | $(or_na "$coll_cp") | $(or_na "$coll_cp_iqr") | $scaling | $(or_na "$coll_v1") | $(or_na "$red_v1") | $(or_na "$red_v2") | $reduce_delta | $(or_na "$tot_v1") | $(or_na "$tot_v2") | $end_to_end | $(or_na "$tot_none") | $(kb_to_mb "${rss_v1:-0}") | $(kb_to_mb "${rss_v2:-0}") | $rss_delta | yes |" >>"$TIME_ROWS"
    echo "entry=$id" >>"$DONE_IDS"
    echo "id=$id C1/C$p=$scaling end-to-end=$end_to_end survivors ${surv_v1:-?} -> ${surv_v2:-?} ($surv_delta) rss=$rss_delta" >&2
done <"$ENTRY_FILE"

if ((ENTRIES == 0)); then
    echo "error: filter ONLY=$ONLY matched no entry in the $SET set" >&2
    exit 1
fi

{
    echo "<!-- Generated by benchmarks/collapse_scaling_rounds.sh. Do not edit; rerun the script. -->"
    echo
    echo "# Parallel edge-collapse scaling, $SET set"
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
    emit_provenance_md
    echo "- corpus: \`$(basename "$CORPUS")\` version $CORPUS_VERSION dated $CORPUS_DATE, $SET set, $ENTRIES entries"
    echo "- driver: \`crates/collapse-bench\`, one process per entry, one clock per phase"
    echo "- repeats: 1 agreement run + at least $REPS timed repetitions per configuration, rounded up per entry to a multiple of its configuration count (the driver's kind=entry line records each count); median and IQR over the timed repetitions"
    echo "- threshold: T = tau * enclosing radius, identical for every configuration"
    echo "- diagrams: compared exactly, bar for bar, across every configuration of the entry before any timing"
    echo "- peak RSS: one extra single-repetition process per pipeline; the driver's own VmHWM covers the whole process and is in the .txt log only"
    echo "- decision rule: \`$(basename "$CORPUS")\`, [meta] decision_rule, quoted in the corpus"
    echo "- entry filter: \`$ONLY\`"
    echo
    echo "## Configuration and collapse yield"
    echo
    echo "P is the entry's last collapse thread count. Survivors are output edges."
    echo
    echo "| id | headline | family | n | dim | maxdim | tau | modulus | collapse threads | reducer threads | density | mean deg | max deg | edges in | survivors v1 | survivors v2 | delta | passes v1 | rounds v2 | edge tests v1 | edge tests v2 | max batch | status |"
    echo "|:--|:--|:--|--:|--:|--:|--:|--:|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|"
    cat "$CFG_ROWS"
    echo
    echo "## Phase medians and the headline ratios"
    echo
    echo "Times are seconds, medians over the timed repetitions. C1/CP is the"
    echo "collapse-only scaling. End to end is the v1 pipeline over the v2 pipeline,"
    echo "both with the same reducer, and above 1.0 means v2 wins. RSS v2/v1 must stay"
    echo "at or below 1.5 for a product-positive grade."
    echo
    echo "| id | headline | collapse v2 c1 | IQR | collapse v2 cP | IQR | C1/CP | collapse v1 | reduce v1 | reduce v2 | reduce v1/v2 | total v1 | total v2 | end to end | total no collapse | RSS v1 (MB) | RSS v2 (MB) | RSS v2/v1 | diagrams identical |"
    echo "|:--|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|"
    cat "$TIME_ROWS"
} >"$RESULTS_MD"

# The manifest is what --confirm checks. It names the corpus the screen
# covered, by version and hash, the commit and filter it ran under, and every
# entry that finished. The run removed any earlier manifest before it wrote
# its first result, so only a clean run that reaches this point leaves one.
# ANY void leaves none: a voided entry, a draft corpus, or a bypassed
# protocol. A void run must not be able to unlock confirmation through a
# manifest that says nothing about why it was void.
if [[ "$SET" == screen ]]; then
    if ((ANY_MISMATCH != 0 || DRAFT != 0)); then
        if ((ANY_MISMATCH != 0)); then
            echo "No screen manifest: this run voided at least one entry." >&2
        fi
        if ((DRAFT != 0)); then
            echo "No screen manifest: this run used an unfrozen draft corpus." >&2
        fi
        echo "This run removed the earlier manifest before it wrote anything, so" >&2
        echo "--confirm stays locked." >&2
    else
    {
        echo "# Written by benchmarks/collapse_scaling_rounds.sh at the end of a clean screen run."
        echo "# --confirm reads it. Do not edit: an edit only fakes a screen."
        echo "corpus_file=$(basename "$CORPUS")"
        echo "corpus_version=$CORPUS_VERSION"
        echo "corpus_date=$CORPUS_DATE"
        echo "corpus_sha256=$(sha256 "$CORPUS")"
        echo "screen_date=$PROV_DATE"
        echo "holos_commit=$PROV_COMMIT"
        echo "filter=$ONLY"
        cat "$DONE_IDS"
    } >"$MANIFEST"
    echo "Screen manifest written to $MANIFEST." >&2
    fi
fi

echo "Results written to $RESULTS and $RESULTS_MD." >&2
echo "Do not copy numbers into documents by hand; rerun this script instead." >&2

if ((ANY_MISMATCH != 0)); then
    echo "FAILURE: at least one entry is void, because the driver failed, an RSS probe failed, or a rotation was not balanced. The timings of that entry do not count." >&2
    exit 1
fi
