#!/usr/bin/env bash
# Break-even study for edge collapse. The corpus is preregistered in
# benchmarks/collapse_corpus.toml; the protocol and the decision rule are in
# benchmarks/README.md. Neither changes in response to a result.
#
# Every entry gets one seeded cloud, reduced three ways from the same
# distances:
#   mode 1  dense path, threshold T
#   mode 2  the same distances thresholded at T and converted to sparse
#           triplets, no collapse: isolates the sparse enumerator
#   mode 3  the same triplets with --collapse-edges
# The three diagrams must be identical before any timing counts. The
# comparison is exact, with no tolerance: collapse must not move an
# endpoint, and all three outputs come from the same f64 printer.
#
# Timings are one warm-up run plus REPS timed runs, reported as median and
# IQR, with peak RSS over the timed runs.
#
# Protocol: run the screen first, then --confirm for the held-out set, after
# appending any densify-near-boundary midpoints the screen triggers. A screen
# run that voids no entry writes results_collapse_manifest.txt, recording the
# corpus version and sha256, the entry filter, and every screen entry it
# finished. --confirm refuses to start unless that manifest covers the whole
# registered screen, ran unfiltered, and the corpus still hashes the same.
#
# Manifest lifecycle: a screen run removes the manifest before it writes the
# first byte of its results, and writes a fresh one only after finishing with
# no voided entry. An interrupted screen therefore leaves partial results and
# no manifest, and --confirm stays locked.
#
# Results: benchmarks/results_collapse.txt (log) and .md (tables). --confirm
# writes results_collapse_confirm.{txt,md} instead, so a confirmation run
# cannot overwrite the screen it depends on. Both are gitignored.
#
# CARGO may carry a toolchain: CARGO="cargo +1.92" ./collapse_bench.sh
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: collapse_bench.sh [--confirm] [-h]

Run the preregistered edge-collapse break-even corpus. Without --confirm it
runs the [[screen]] entries. With --confirm it runs the held-out
[[confirm]] entries, which needs a complete screen run of this same corpus
first.

Environment:
  CARGO           cargo invocation (may carry a toolchain), default "cargo"
  CORPUS          corpus file, default benchmarks/collapse_corpus.toml
  REPS            timed runs per mode, default 5 (the preregistered minimum)
  ONLY            glob over entry ids, default all. A filtered screen run
                  cannot unlock --confirm: the manifest records the filter,
                  and --confirm takes the full corpus only
  ALLOW_DIRTY     set to 1 to benchmark a dirty worktree (recorded as -DIRTY)
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

CORPUS="${CORPUS:-$HERE/collapse_corpus.toml}"
REPS="${REPS:-5}"
ONLY="${ONLY:-*}"
# Exact diagram equality. compare_diagrams reads TOLERANCE.
TOLERANCE=0

if ((REPS < 5)); then
    echo "error: REPS=$REPS is below the preregistered minimum of 5" >&2
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

SCREEN_RESULTS="$HERE/results_collapse.txt"
MANIFEST="$HERE/results_collapse_manifest.txt"
RESULTS="$SCREEN_RESULTS"
RESULTS_MD="$HERE/results_collapse.md"
if [[ "$SET" == confirm ]]; then
    RESULTS="$HERE/results_collapse_confirm.txt"
    RESULTS_MD="$HERE/results_collapse_confirm.md"
fi

# corpus_meta : "version=<v> date=<d>" from [meta].
corpus_meta() {
    python3 - "$CORPUS" <<'EOF'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    meta = tomllib.load(f).get("meta", {})
print(f"version={meta.get('version', '?')} date={meta.get('date', '?')}")
EOF
}

# corpus_entries TABLE : one tab-separated row per entry, fields in the order
# the loop below reads them. Fails on a missing field or a duplicate id, so a
# corpus edit cannot silently drop an axis.
corpus_entries() {
    python3 - "$CORPUS" "$1" <<'EOF'
import sys
import tomllib

path, table = sys.argv[1], sys.argv[2]
with open(path, "rb") as f:
    corpus = tomllib.load(f)
entries = corpus.get(table, [])
if not entries:
    sys.exit(f"corpus has no [[{table}]] entries")
fields = ("id", "family", "n", "coord_dim", "max_dim", "tau", "modulus", "threads", "seed")
seen = set()
for entry in entries:
    missing = [k for k in fields if k not in entry]
    if missing:
        sys.exit(f"entry {entry.get('id', '?')} lacks: {', '.join(missing)}")
    if entry["id"] in seen:
        sys.exit(f"duplicate entry id {entry['id']}")
    seen.add(entry["id"])
    print("\t".join(str(entry[k]) for k in fields))
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

META="$(corpus_meta)"
CORPUS_VERSION="$(field version "$META")"
CORPUS_DATE="$(field date "$META")"

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

HEADER="holos edge-collapse break-even run ($SET set)"
if ((BYPASS != 0)); then
    HEADER="SCREEN-PROTOCOL-BYPASSED: no verified screen manifest backs this run.
Its numbers are void for the decision rule in benchmarks/README.md.
$HEADER"
fi

mkdir -p "$DATA"
build_holos

# Remove the old manifest before the first byte of results is written. A crash
# between here and the end of the run then leaves partial results and no
# manifest, instead of a complete manifest that vouches for them.
if [[ "$SET" == screen ]]; then
    rm -f "$MANIFEST"
fi
emit_provenance "$RESULTS" "$HEADER"

{
    echo "corpus: $(basename "$CORPUS") version $CORPUS_VERSION dated $CORPUS_DATE"
    echo "set: $SET  entries filtered by: $ONLY"
    echo "repeats: 1 warm-up + $REPS timed runs per mode; median, IQR, peak RSS over the timed runs"
    echo "diagram comparison: exact (tolerance $TOLERANCE)"
    echo "mode 1: dense lower-distance, threshold T = tau * enclosing radius"
    echo "mode 2: same distances as sparse triplets, threshold T, no collapse"
    echo "mode 3: same triplets, threshold T, --collapse-edges"
    echo
} >>"$RESULTS"

CFG_ROWS="$DATA/collapse_rows_cfg_$SET.md"
TIME_ROWS="$DATA/collapse_rows_time_$SET.md"
ENTRY_FILE="$DATA/collapse_entries_$SET.tsv"
DONE_IDS="$DATA/collapse_done_$SET.txt"
: >"$CFG_ROWS"
: >"$TIME_ROWS"
: >"$DONE_IDS"
corpus_entries "$SET" >"$ENTRY_FILE"

ANY_MISMATCH=0
ENTRIES=0

while IFS=$'\t' read -r id family n coord_dim max_dim tau modulus threads seed; do
    if [[ "$id" != $ONLY ]]; then
        continue
    fi
    ENTRIES=$((ENTRIES + 1))

    cloud="$DATA/collapse_${SET}_${id}.csv"
    lower="$DATA/collapse_${SET}_${id}.lower"
    sparse="$DATA/collapse_${SET}_${id}.sparse"
    python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
    graph="$(python3 "$HERE/densify_to_sparse.py" "$cloud" "$tau" "$sparse" "$lower")"

    threshold="$(field threshold "$graph")"
    edges_in="$(field edges "$graph")"
    density="$(field density "$graph")"
    mean_degree="$(field mean_degree "$graph")"
    max_degree="$(field max_degree "$graph")"
    isolated="$(field isolated "$graph")"

    mode1=("$HOLOS_BIN" "$lower" --format lower-distance --dim "$max_dim"
        --threshold "$threshold" --modulus "$modulus" --threads "$threads")
    mode2=("$HOLOS_BIN" "$sparse" --format sparse --dim "$max_dim"
        --threshold "$threshold" --modulus "$modulus" --threads "$threads")
    mode3=("${mode2[@]}" --collapse-edges)

    out1="$DATA/collapse_${SET}_${id}_m1.out"
    out2="$DATA/collapse_${SET}_${id}_m2.out"
    out3="$DATA/collapse_${SET}_${id}_m3.out"
    err1="$DATA/collapse_${SET}_${id}_m1.err"
    err2="$DATA/collapse_${SET}_${id}_m2.err"
    err3="$DATA/collapse_${SET}_${id}_m3.err"

    # Agreement first. These runs also warm the page cache. A mode that exits
    # nonzero voids this entry, exactly as a diagram mismatch does, and the
    # remaining entries still run.
    failed=""
    if ! measure_err "$out1" "$err1" "${mode1[@]}" >/dev/null; then
        failed="mode1"
    elif ! measure_err "$out2" "$err2" "${mode2[@]}" >/dev/null; then
        failed="mode2"
    elif ! measure_err "$out3" "$err3" "${mode3[@]}" >/dev/null; then
        failed="mode3"
    fi
    match2="n/a"
    match3="n/a"
    if [[ -z "$failed" ]]; then
        match2="$(compare_diagrams "$out1" "$out2")"
        match3="$(compare_diagrams "$out1" "$out3")"
    fi

    # "collapse: kept X of Y edges, removed Z, N passes"
    collapse_line="$(grep -m1 '^collapse:' "$err3" || true)"
    if [[ -n "$collapse_line" ]]; then
        read -r kept collapse_in removed passes < <(
            awk '{ gsub(/,/, ""); print $3, $5, $8, $9 }' <<<"$collapse_line"
        )
    else
        kept="n/a"
        collapse_in="n/a"
        removed="n/a"
        passes="n/a"
    fi

    {
        echo "== id=$id family=$family n=$n coord_dim=$coord_dim max_dim=$max_dim tau=$tau modulus=$modulus threads=$threads seed=$seed"
        echo "graph: $graph"
        # Recorded commands stay repo-relative, as the provenance rule in
        # _common.sh demands.
        echo "mode1 cmd: ${mode1[*]//$ROOT\//}"
        echo "mode2 cmd: ${mode2[*]//$ROOT\//}"
        echo "mode3 cmd: ${mode3[*]//$ROOT\//}"
        echo "collapse stats: ${collapse_line:-<no collapse line on stderr>}"
        echo "DIAGRAMS_MATCH mode2=$match2 mode3=$match3"
    } >>"$RESULTS"

    if [[ "$collapse_in" != "n/a" && "$collapse_in" != "$edges_in" ]]; then
        echo "note: collapse saw $collapse_in input edges, the converted graph has $edges_in" >>"$RESULTS"
    fi

    if [[ -n "$failed" || "$match2" != yes || "$match3" != yes ]]; then
        ANY_MISMATCH=1
        {
            if [[ -n "$failed" ]]; then
                echo "VOID: $failed exited nonzero, no timings recorded for this entry"
                sed 's/^/  /' "$err1" "$err2" "$err3" 2>/dev/null || true
            else
                echo "VOID: diagrams disagree, no timings recorded for this entry"
            fi
            echo
        } >>"$RESULTS"
        echo "| $id | $family | $n | $coord_dim | $max_dim | $tau | $modulus | $threads | $density | $mean_degree | $max_degree | $isolated | $edges_in | $kept | $removed | $passes | VOID |" >>"$CFG_ROWS"
        echo "| $id | void | void | void | void | void | void | void | void | void | void | void | no |" >>"$TIME_ROWS"
        echo "id=$id VOID (${failed:+$failed exited nonzero, }match2=$match2 match3=$match3)" >&2
        continue
    fi

    # A timed run that exits nonzero voids the entry too. Without the guard,
    # errexit would end the whole run on one bad mode.
    if ! stats1="$(measure_repeat "$out1" "$err1" "$REPS" "${mode1[@]}")"; then
        failed="mode1"
    elif ! stats2="$(measure_repeat "$out2" "$err2" "$REPS" "${mode2[@]}")"; then
        failed="mode2"
    elif ! stats3="$(measure_repeat "$out3" "$err3" "$REPS" "${mode3[@]}")"; then
        failed="mode3"
    fi
    if [[ -n "$failed" ]]; then
        ANY_MISMATCH=1
        {
            echo "VOID: $failed exited nonzero during the timed runs; this entry is void"
            echo
        } >>"$RESULTS"
        echo "| $id | $family | $n | $coord_dim | $max_dim | $tau | $modulus | $threads | $density | $mean_degree | $max_degree | $isolated | $edges_in | $kept | $removed | $passes | VOID |" >>"$CFG_ROWS"
        echo "| $id | void | void | void | void | void | void | void | void | void | void | void | no |" >>"$TIME_ROWS"
        echo "id=$id VOID ($failed exited nonzero during the timed runs)" >&2
        continue
    fi

    med1="$(field median_s "$stats1")"
    med2="$(field median_s "$stats2")"
    med3="$(field median_s "$stats3")"
    iqr1="$(field iqr_s "$stats1")"
    iqr2="$(field iqr_s "$stats2")"
    iqr3="$(field iqr_s "$stats3")"
    rss1="$(field max_rss_kb "$stats1")"
    rss2="$(field max_rss_kb "$stats2")"
    rss3="$(field max_rss_kb "$stats3")"

    sp31="$(speedup "$med1" "$med3")"
    sp32="$(speedup "$med2" "$med3")"

    {
        echo "mode1 $stats1"
        echo "mode2 $stats2"
        echo "mode3 $stats3"
        echo "SPEEDUP mode3_vs_mode1=$sp31 mode3_vs_mode2=$sp32"
        echo
    } >>"$RESULTS"

    echo "| $id | $family | $n | $coord_dim | $max_dim | $tau | $modulus | $threads | $density | $mean_degree | $max_degree | $isolated | $edges_in | $kept | $removed | $passes | ok |" >>"$CFG_ROWS"
    echo "| $id | $med1 | $iqr1 | $med2 | $iqr2 | $med3 | $iqr3 | $(kb_to_mb "$rss1") | $(kb_to_mb "$rss2") | $(kb_to_mb "$rss3") | $sp31 | $sp32 | yes |" >>"$TIME_ROWS"
    echo "entry=$id" >>"$DONE_IDS"
    echo "id=$id edges $edges_in -> $kept  m1=${med1}s m2=${med2}s m3=${med3}s  m3/m1=$sp31 m3/m2=$sp32" >&2
done <"$ENTRY_FILE"

if ((ENTRIES == 0)); then
    echo "error: filter ONLY=$ONLY matched no entry in the $SET set" >&2
    exit 1
fi

{
    echo "<!-- Generated by benchmarks/collapse_bench.sh. Do not edit; rerun the script. -->"
    echo
    echo "# Edge-collapse break-even, $SET set"
    echo
    if ((BYPASS != 0)); then
        echo "**SCREEN-PROTOCOL-BYPASSED.** ALLOW_NO_SCREEN=1 skipped the screen"
        echo "manifest check, so no verified screen backs this run. Its numbers are"
        echo "void for the decision rule."
        echo
    fi
    emit_provenance_md
    echo "- corpus: \`$(basename "$CORPUS")\` version $CORPUS_VERSION dated $CORPUS_DATE, $SET set, $ENTRIES entries"
    echo "- repeats: 1 warm-up + $REPS timed runs per mode; median and IQR over the timed runs"
    echo "- threshold: T = tau * enclosing radius, identical for all three modes"
    echo "- diagram comparison: exact, tolerance $TOLERANCE; a mismatch voids the entry"
    echo "- decision rule: benchmarks/README.md, 'Preregistration: edge-collapse break-even study'"
    echo
    echo "## Configuration and collapse yield"
    echo
    echo "| id | family | n | dim | maxdim | tau | p | threads | density | mean deg | max deg | isolated | edges in | edges out | removed | passes | status |"
    echo "|:--|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|"
    cat "$CFG_ROWS"
    echo
    echo "## Wall time, memory, and speedup"
    echo
    echo "mode 1 = dense, mode 2 = sparse without collapse, mode 3 = sparse with collapse."
    echo "Times are seconds. Speedups are median over median, above 1.0 when collapse wins."
    echo
    echo "| id | m1 median | m1 IQR | m2 median | m2 IQR | m3 median | m3 IQR | m1 RSS (MB) | m2 RSS (MB) | m3 RSS (MB) | m3 vs m1 | m3 vs m2 | diagrams identical |"
    echo "|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|:--|"
    cat "$TIME_ROWS"
} >"$RESULTS_MD"

# The manifest is what --confirm checks. It names the corpus the screen
# covered, by version and hash, the filter it ran under, and every entry that
# finished with matching diagrams. The run removed any earlier manifest before
# it wrote its first result, so only a clean run that reaches this point
# leaves one. A run that voided an entry leaves none.
if [[ "$SET" == screen ]]; then
    if ((ANY_MISMATCH != 0)); then
        echo "No screen manifest: this run voided at least one entry." >&2
        echo "This run removed the earlier manifest before it wrote anything, so" >&2
        echo "--confirm stays locked." >&2
    else
        {
            echo "# Written by benchmarks/collapse_bench.sh at the end of a clean screen run."
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
    echo "FAILURE: at least one entry's three modes disagree; that entry's timings are void." >&2
    exit 1
fi
