#!/usr/bin/env bash
# Engineering benchmark for the serial reduction: holos against ripser on
# identical inputs. Not a registered study. Its numbers tune the engine and
# decide whether a change lands; no public claim may cite them.
#
# The corpus is benchmarks/engineering_corpus.toml. It holds two sets. The
# tuning set is disclosed: read it, rerun it, and pick constants from it.
# The landing set is disjoint in seed and in size, and it runs once, after
# the constants are frozen, to decide landing. Every entry names a stratum,
# and the summary reports a median per stratum before any overall median.
#
# The primary total is a fresh process. holos and ripser both start, read
# the same file at the same threshold, and exit; benchmarks/measure.py
# times them and reads their peak RSS. Both get one warm-up run and the
# same number of timed repetitions. The in-process driver
# crates/engine-bench runs beside them and supplies the phase clocks, which
# are a diagnostic table and not the headline.
#
# Arms. HOLOS_ARMS names one or more holos builds as label=commit, so one
# run can time the working tree beside earlier commits. Every arm runs
# every entry, and the tables carry one column per arm and configuration.
# Each historical arm gets a checkout and a target directory of its own
# under HIST_DIR, so no arm can overwrite another's artifacts. Four arms
# hold four full builds, which is a few gigabytes; put HIST_DIR on a large
# filesystem, and on this machine that is tmpfs. CARGO_TARGET_DIR moves the
# working tree's own build the same way.
#
# Configurations. An arm whose holos takes `--engine auto|dense|sparse`
# runs auto, forced-dense, and forced-sparse on one input file, which is
# the routing axis. It also runs sparse-file, where both tools read the
# triplet file: holos with `--format sparse --engine auto`, ripser with
# `--format sparse`, at the same threshold and the same dimension. The
# routing axis holds the file fixed and varies the engine. sparse-file
# holds the format fixed, so every stratum carries one sparse-input ratio
# that compares like with like. An older arm has no routing flag, so its
# axis is the input file instead:
#   dense    holos and ripser read the lower-distance file
#   sparse   holos and ripser read the triplet file, --format sparse
# A graph entry has no dense file, so every configuration of it reads the
# triplet file. This script probes each arm's --help and picks the axis per
# arm. ONLY_CONFIGS runs a subset of the names, so one configuration can be
# timed again without repeating the rest.
#
# Gate. A change lands when two conditions hold, for the routing ratios and
# for the sparse-file ratios alike: every headline stratum median is at or
# below 1.0, and no entry is above 1.05 outside the frozen noise rule. An
# entry that runs in milliseconds carries no strict requirement; its spread
# is wider than the effect.
#
# The storage axis is not wired in here. Adding compact and square as
# configurations doubles every dense run and changes the shape of the
# record. This script times the shipped storage rule. Storage tuning runs
# outside it.
#
# Every entry gets one seeded input and one threshold. Cloud entries use
# gen_cloud.py and densify_to_sparse.py, which writes the lower-distance
# file and the triplet file from the same distances. Graph entries use
# gen_graph.py, which writes the triplet file alone. An entry with
# collapse = true has its graph reduced by the certified serial edge
# collapse first; the barcode does not change, and the reduced graph is
# then a native sparse input.
#
# Agreement. The driver computes the entry's reference diagram and checks
# its own engine entry points against each other bar for bar. Every arm and
# every ripser run is then compared against that reference within the
# tolerance. A mismatch voids the entry.
#
# Results: benchmarks/results_engine_tuning.txt (log) and .md (tables).
# --landing writes results_engine_landing.{txt,md} instead. Both are
# gitignored.
#
# CARGO may carry a toolchain: CARGO="cargo +1.92" ./engine_bench.sh
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: engine_bench.sh [--landing] [-h]

Run the engineering corpus for the serial reduction. Without --landing the
script runs the [[tuning]] entries. With --landing it runs the held-back
[[landing]] entries, which decide whether a change ships.

Environment:
  RIPSER_BIN      path to a ripser binary (required)
  CARGO           cargo invocation (may carry a toolchain), default "cargo"
  CORPUS          corpus file, default benchmarks/engineering_corpus.toml
  HOLOS_ARMS      space-separated label=commit list of holos builds,
                  default "tree=@". "@" means the working tree. Any other
                  value is a git commit; the script checks it out under
                  HIST_DIR/<sha>/ with `git worktree add --detach`, builds
                  holos-tda there into that checkout's own target
                  directory, and records the sha of every binary in the
                  provenance header
  HIST_DIR        where the historical checkouts and their target
                  directories live, default benchmarks/data/hist. One arm
                  holds one full build, so point it at a filesystem with
                  room to spare
  CARGO_TARGET_DIR
                  cargo's own variable, honoured here: it moves the
                  working tree's build out of ./target the same way
  REBUILD_HIST    set to 1 to rebuild the historical checkouts even when
                  their binaries already exist
  REPS            timed fresh-process repetitions per arm and per
                  configuration, default 5. Every arm and ripser get the
                  same count, after one warm-up run
  DRIVER_REPS     minimum timed repetitions inside the driver, default 3.
                  Each entry runs the smallest multiple of its engine
                  entry point count at or above it, so the rotation stays
                  balanced. These are the diagnostic phase clocks
  ONLY            comma-separated globs over entry ids, default all
  ONLY_CONFIGS    comma-separated configuration names, default all. The
                  names are auto, forced-dense, forced-sparse, and
                  sparse-file on an arm that takes --engine, and dense and
                  sparse on an older arm. Use it to time one configuration
                  again; the record names the filter
  TOLERANCE       absolute agreement tolerance, default 1e-5
  ALLOW_DIRTY     set to 1 to benchmark a dirty worktree (recorded as -DIRTY)
  ALLOW_RERUN     set to 1 to overwrite an existing landing record. The
                  landing set decides landing once; a rerun is recorded as
                  LANDING-RERUN in the header

This is an engineering instrument. It has no decision rule, no manifest,
and no protocol gate. It reports timings, peak RSS, and agreement.
EOF
}

SET=tuning
for arg in "$@"; do
    case "$arg" in
        --landing) SET=landing ;;
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

CORPUS="${CORPUS:-$HERE/engineering_corpus.toml}"
HIST_DIR="${HIST_DIR:-$DATA/hist}"
REPS="${REPS:-5}"
DRIVER_REPS="${DRIVER_REPS:-3}"
ONLY="${ONLY:-*}"
ONLY_CONFIGS="${ONLY_CONFIGS:-all}"
TOLERANCE="${TOLERANCE:-1e-5}"
HOLOS_ARMS="${HOLOS_ARMS:-tree=@}"
export TOLERANCE

if [[ -z "${RIPSER_BIN:-}" ]]; then
    cat >&2 <<'EOF'
RIPSER_BIN is not set.

Point it at a ripser binary (https://github.com/Ripser/ripser):

    git clone https://github.com/Ripser/ripser
    make -C ripser
    RIPSER_BIN=ripser/ripser ./engine_bench.sh
EOF
    exit 1
fi
if [[ ! -x "$RIPSER_BIN" ]]; then
    echo "error: RIPSER_BIN=$RIPSER_BIN is not an executable file" >&2
    exit 1
fi

for name in REPS DRIVER_REPS; do
    if ! [[ "${!name}" =~ ^[0-9]+$ ]] || ((${!name} < 1)); then
        echo "error: $name=${!name} is not a positive integer" >&2
        exit 1
    fi
done

KNOWN_CONFIGS="auto forced-dense forced-sparse sparse-file dense sparse"
if [[ "$ONLY_CONFIGS" != all ]]; then
    IFS=, read -r -a ONLY_CONFIG_NAMES <<<"$ONLY_CONFIGS"
    for cfg_name in "${ONLY_CONFIG_NAMES[@]}"; do
        case " $KNOWN_CONFIGS " in
            *" $cfg_name "*) ;;
            *)
                echo "error: ONLY_CONFIGS names no configuration: $cfg_name" >&2
                echo "       the configurations are: $KNOWN_CONFIGS" >&2
                exit 1
                ;;
        esac
    done
fi

if [[ ! -r "$CORPUS" ]]; then
    echo "error: no corpus at $(basename "$CORPUS")" >&2
    exit 1
fi

if ! python3 -c 'import tomllib' 2>/dev/null; then
    echo "error: reading the corpus needs Python 3.11 or newer (tomllib)" >&2
    exit 1
fi

RESULTS="$HERE/results_engine_$SET.txt"
RESULTS_MD="$HERE/results_engine_$SET.md"

# The landing set is the one untouched measurement of a change. A second
# run of it after seeing the first is no longer untouched, so overwriting
# its record takes an explicit override, and the override is disclosed.
RERUN=0
if [[ "$SET" == landing && -s "$RESULTS" ]]; then
    if [[ "${ALLOW_RERUN:-}" == "1" ]]; then
        RERUN=1
        echo "warning: ALLOW_RERUN=1 overwrites an earlier landing record." >&2
        echo "         The landing set is no longer untouched, and the header says so." >&2
    else
        echo "error: a landing record already exists at $(basename "$RESULTS")" >&2
        echo "       The landing set decides landing once. Move the old record aside," >&2
        echo "       or set ALLOW_RERUN=1 to overwrite it and disclose the rerun." >&2
        exit 1
    fi
fi
if [[ "$SET" == landing && "$ONLY" != "*" ]]; then
    echo "warning: ONLY=$ONLY runs part of the landing set." >&2
    echo "         A filtered landing run decides nothing; the record names the filter." >&2
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

# corpus_entries TABLE : one tab-separated row per entry, fields in the
# order the loop below reads them. Fails on a missing field, an unknown
# field, or a duplicate id, so a corpus edit cannot silently drop an axis.
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

COMMON = ("id", "stratum", "input", "n", "max_dim", "tau", "modulus", "seed",
          "headline", "competitor")
BY_INPUT = {"cloud": ("family", "coord_dim"), "graph": ("generator", "param")}
OPTIONAL = ("collapse", "driver_modes")
ORDER = ("id", "stratum", "input", "family", "generator", "n", "coord_dim",
         "param", "max_dim", "tau", "modulus", "seed", "headline",
         "competitor", "collapse", "driver_modes")

seen = set()
for entry in entries:
    name = entry.get("id", "?")
    kind = entry.get("input")
    if kind not in BY_INPUT:
        sys.exit(f"entry {name}: input must be cloud or graph, got {kind!r}")
    required = COMMON + BY_INPUT[kind]
    missing = [k for k in required if k not in entry]
    if missing:
        sys.exit(f"entry {name} lacks: {', '.join(missing)}")
    extra = set(entry) - set(required) - set(OPTIONAL)
    if extra:
        sys.exit(f"entry {name} carries unknown fields: {', '.join(sorted(extra))}")
    if entry["competitor"] not in ("ripser", "none"):
        sys.exit(f"entry {name}: competitor must be ripser or none")
    if entry["id"] in seen:
        sys.exit(f"duplicate entry id {entry['id']}")
    seen.add(entry["id"])

    collapse = bool(entry.get("collapse", False))
    # A collapsed input is a graph whatever it grew from: the reduced
    # graph is written as triplets and no dense file of it exists.
    modes = entry.get("driver_modes")
    if modes is None:
        dense_file = kind == "cloud" and not collapse
        modes = "dense,sparse,auto" if dense_file else "sparse"
    row = {
        "family": "-",
        "generator": "-",
        "coord_dim": 0,
        "param": 0,
        "collapse": "yes" if collapse else "no",
        "driver_modes": modes,
    }
    row.update({k: entry[k] for k in required})
    row["headline"] = "yes" if entry["headline"] else "no"
    print("\t".join(str(row[k]) for k in ORDER))
EOF
}

# build_tree : release build of the working tree's holos binary and of the
# in-process driver. HOLOS_BIN is the CLI binary, so the provenance header
# names what the primary total measures. The display path stays relative,
# as the provenance rule in _common.sh demands.
build_tree() {
    $CARGO build --release -p holos-tda -p engine-bench --manifest-path "$ROOT/Cargo.toml" >&2
    local dir="${CARGO_TARGET_DIR:-$ROOT/target}"
    HOLOS_BIN="$dir/release/holos"
    HOLOS_BIN_DISPLAY="target/release/holos"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        HOLOS_BIN_DISPLAY='<CARGO_TARGET_DIR>/release/holos'
    fi
    BUILD_CMD_DISPLAY="$CARGO build --release -p holos-tda -p engine-bench"
    DRIVER_BIN="$dir/release/engine-bench"
}

# resolve_arms : one holos build per HOLOS_ARMS item. A historical arm gets
# a detached checkout of its commit under HIST_DIR/<sha>/ and is built
# there, into that checkout's own target directory, so no arm can overwrite
# another's artifacts. An existing binary is reused unless REBUILD_HIST=1.
resolve_arms() {
    ARM_LABEL=()
    ARM_BIN=()
    ARM_COMMIT=()
    ARM_SHA=()
    ARM_VERSION=()
    ARM_CONFIGS=()
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
        # The record keys its rows by arm name, and two names are taken.
        if [[ "$label" == ripser || "$label" == driver ]]; then
            echo "error: arm label '$label' is reserved for the record" >&2
            exit 1
        fi
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
                $CARGO build --release -p holos-tda \
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
        ARM_CONFIGS+=("$(routing_configs "${ARM_BIN[-1]}")")
    done
}

# routing_configs BIN : "routing" when the binary takes the engine-routing
# flag `--engine auto|dense|sparse`, "files" when it does not. The working
# tree carries the flag, so it runs the three routing configurations on one
# file. An arm built from a commit older than routing reports files and
# runs the two input files instead, which is the same measurement its
# library can make.
routing_configs() {
    if "$1" --help 2>&1 | grep -q -- '--engine'; then
        echo routing
    else
        echo files
    fi
}

# entry_configs ARM_INDEX INPUT_KIND : the configurations this arm runs on
# this entry, space separated, after the ONLY_CONFIGS filter.
entry_configs() {
    local all cfg out=""
    if [[ "${ARM_CONFIGS[$1]}" == routing ]]; then
        all="auto forced-dense forced-sparse sparse-file"
    elif [[ "$2" == cloud ]]; then
        all="dense sparse"
    else
        all="sparse"
    fi
    for cfg in $all; do
        if [[ "$ONLY_CONFIGS" == all ]]; then
            out="$out $cfg"
        else
            case ",$ONLY_CONFIGS," in
                *",$cfg,"*) out="$out $cfg" ;;
            esac
        fi
    done
    echo "${out# }"
}

# config_input CONFIG : set cfg_file, cfg_format, cfg_engine, and
# engine_flag for one configuration of the current entry. The routing
# configurations all read the entry's primary file and differ only in the
# flag. The file configurations differ only in the file. sparse-file reads
# the triplet file with routing left on auto, which is the honest choice: a
# native sparse input goes to the sparse engine anyway. On an entry whose
# primary file is already the triplet file, sparse-file is the auto
# configuration again; the arm loop keys its timings by file and engine, so
# it records both rows from one run rather than time one command twice.
config_input() {
    case "$1" in
        dense)
            cfg_file="$lower"
            cfg_format="lower-distance"
            cfg_engine=none
            engine_flag=()
            ;;
        sparse)
            cfg_file="$sparse"
            cfg_format="sparse"
            cfg_engine=none
            engine_flag=()
            ;;
        sparse-file)
            cfg_file="$sparse"
            cfg_format="sparse"
            cfg_engine=auto
            engine_flag=(--engine auto)
            ;;
        auto | forced-dense | forced-sparse)
            cfg_file="$primary_file"
            cfg_format="$primary_format"
            cfg_engine="${1#forced-}"
            engine_flag=(--engine "${1#forced-}")
            ;;
        *)
            echo "error: unknown configuration $1" >&2
            exit 1
            ;;
    esac
}

# record_cmd CMD... : the command as the record shows it. A path inside the
# repo becomes repo-relative, a build directory outside it becomes its
# placeholder, and the ripser binary keeps its basename. The provenance rule
# in _common.sh forbids an absolute path, a home directory, or a username in
# a record.
record_cmd() {
    local text="$*"
    text="${text//$RIPSER_BIN/$RIPSER_BIN_DISPLAY}"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        text="${text//$CARGO_TARGET_DIR/<CARGO_TARGET_DIR>}"
    fi
    text="${text//$HIST_DIR/<HIST_DIR>}"
    echo "${text//$ROOT\//}"
}

# ratio A B -> A / B, "n/a" when either is missing or zero.
ratio() {
    awk -v a="${1:-}" -v b="${2:-}" 'BEGIN {
        if (a + 0 == 0 || b + 0 == 0) print "n/a"; else printf "%.2f", a / b
    }'
}

or_na() {
    if [[ -n "${1:-}" ]]; then echo "$1"; else echo "n/a"; fi
}

# rss_kb STATS : the peak RSS of one probe in kB, empty when the probe
# measured nothing. measure.py prints 0 when it never caught the child
# after exec, so 0 is a miss and never a reading.
rss_kb() {
    local kb
    kb="$(field max_rss_kb "${1:-}")"
    if [[ -n "$kb" && "$kb" != 0 ]]; then
        echo "$kb"
    fi
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

META="$(corpus_meta)"
CORPUS_VERSION="$(field version "$META")"
CORPUS_DATE="$(field date "$META")"

mkdir -p "$DATA"
ENTRY_FILE="$DATA/engine_entries_$SET.tsv"
TOTALS="$DATA/engine_totals_$SET.tsv"
PHASES="$DATA/engine_phases_$SET.tsv"
ENTRY_META="$DATA/engine_entry_meta_$SET.tsv"
: >"$TOTALS"
: >"$PHASES"
: >"$ENTRY_META"
corpus_entries "$SET" >"$ENTRY_FILE"

HEADER="holos engineering benchmark, serial reduction against ripser ($SET set)
ENGINEERING RUN, NOT A REGISTERED STUDY. These numbers tune the engine and
decide landing. No public claim may cite them."
if ((RERUN != 0)); then
    HEADER="LANDING-RERUN: ALLOW_RERUN=1 overwrote an earlier landing record,
so this set is no longer untouched.
$HEADER"
fi

build_tree
resolve_arms
RIPSER_BIN_DISPLAY="$(basename "$RIPSER_BIN")"
RIPSER_SHA="$(sha256 "$RIPSER_BIN")"
# ripser's compile flags, if a Makefile sits next to the binary (a vendored
# build). Otherwise they are unknowable from here.
RIPSER_DIR="$(cd "$(dirname "$RIPSER_BIN")" && pwd)"
if [[ -f "$RIPSER_DIR/Makefile" ]]; then
    RIPSER_FLAGS="$(awk '/^ripser:/ { getline; sub(/^\t+/, ""); print; exit }' "$RIPSER_DIR/Makefile") [from the Makefile beside the binary]"
else
    RIPSER_FLAGS="unknown (no Makefile beside the binary)"
fi

ROUTING_STATE=files
for state in "${ARM_CONFIGS[@]}"; do
    if [[ "$state" == routing ]]; then
        ROUTING_STATE=mixed
    fi
done

# The tables are written once, at the end. Remove the old ones now, so a
# crash cannot leave a complete-looking table from an earlier run beside a
# partial log.
rm -f "$RESULTS_MD"
emit_provenance "$RESULTS" "$HEADER"

{
    echo "corpus: $(basename "$CORPUS") version $CORPUS_VERSION dated $CORPUS_DATE"
    echo "set: $SET  entries filtered by: $ONLY  configurations filtered by: $ONLY_CONFIGS"
    for i in "${!ARM_LABEL[@]}"; do
        echo "arm ${ARM_LABEL[$i]}: commit ${ARM_COMMIT[$i]} sha256 ${ARM_SHA[$i]} version ${ARM_VERSION[$i]} configurations ${ARM_CONFIGS[$i]}"
    done
    echo "ripser binary: $RIPSER_BIN_DISPLAY"
    echo "ripser sha256: $RIPSER_SHA"
    echo "ripser build: $RIPSER_FLAGS"
    echo "primary total: fresh process per run, benchmarks/measure.py, 1 warm-up + $REPS timed repetitions per arm and configuration; ripser gets the same"
    echo "diagnostic: crates/engine-bench, one process per entry, one clock per phase, at least $DRIVER_REPS timed repetitions rounded up to a multiple of the entry's engine entry point count"
    echo "configurations: $ROUTING_STATE (routing means --engine auto|dense|sparse on one file, plus sparse-file, where holos and ripser both read the triplet file; files means the dense and the sparse input file)"
    echo "input: one seeded input per entry; cloud entries carry a lower-distance file and a triplet file built from the same distances, graph entries carry a triplet file"
    echo "agreement: the driver checks its engine entry points against each other bar for bar, then every arm and every ripser run is compared against that reference within $TOLERANCE"
    echo "peak RSS: from the timed fresh-process runs, plus one extra single-repetition driver process per engine entry point"
    echo "reporting: median per stratum first; an overall median never stands alone"
    echo
} >>"$RESULTS"

ANY_MISMATCH=0
ENTRIES=0

# selected ID : true when ID matches one of the ONLY globs. The patterns
# come out of `read -a`, which splits without expanding them against the
# working directory; a bare "*" in a for loop would list files instead.
selected() {
    local pattern
    for pattern in "${ONLY_PATTERNS[@]}"; do
        if [[ "$1" == $pattern ]]; then
            return 0
        fi
    done
    return 1
}
IFS=, read -r -a ONLY_PATTERNS <<<"$ONLY"

while IFS=$'\t' read -r id stratum input family generator n coord_dim param max_dim tau modulus seed headline competitor collapse driver_modes; do
    if ! selected "$id"; then
        continue
    fi
    ENTRIES=$((ENTRIES + 1))

    cloud="$DATA/engine_${SET}_${id}.csv"
    lower="$DATA/engine_${SET}_${id}.lower"
    sparse="$DATA/engine_${SET}_${id}.sparse"
    reference="$DATA/engine_${SET}_${id}_ref.out"
    rows="$DATA/engine_${SET}_${id}.rows"
    errf="$DATA/engine_${SET}_${id}.err"

    if [[ "$input" == cloud ]]; then
        python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
        graph="$(python3 "$HERE/densify_to_sparse.py" "$cloud" "$tau" "$sparse" "$lower")"
        source_kind="cloud $family coord_dim=$coord_dim"
    else
        graph="$(python3 "$HERE/gen_graph.py" "$generator" "$n" "$seed" "$tau" "$param" "$sparse")"
        source_kind="graph $generator param=$param"
    fi
    threshold="$(field threshold "$graph")"
    edges_in="$(field edges "$graph")"

    # An entry with collapse = true measures a real collapsed graph. The
    # collapse runs once, untimed, and its output replaces the input. The
    # barcode is unchanged, so the reference and ripser still agree.
    collapse_note="none"
    if [[ "$collapse" == yes ]]; then
        collapsed="$DATA/engine_${SET}_${id}.collapsed"
        collapse_note="$("$DRIVER_BIN" --entry "$id" --input "$sparse" --format sparse \
            --threshold "$threshold" --emit-collapsed "$collapsed")"
        sparse="$collapsed"
        edges_in="$(field edges_out "$collapse_note")"
    fi

    # The driver reads the dense file when the entry has one, because that
    # is the input its two engine entry points share. A graph entry, and
    # any collapsed entry, has only the triplet file.
    file_kind="$input"
    if [[ "$collapse" == yes ]]; then
        file_kind=graph
    fi
    if [[ "$file_kind" == cloud ]]; then
        driver_input="$lower"
        driver_format="lower-distance"
        primary_file="$lower"
        primary_format="lower-distance"
    else
        driver_input="$sparse"
        driver_format="sparse"
        primary_file="$sparse"
        primary_format="sparse"
    fi

    modes_n="$(awk -F, '{print NF}' <<<"$driver_modes")"
    driver_reps=$(((DRIVER_REPS + modes_n - 1) / modes_n * modes_n))
    driver_base=("$DRIVER_BIN" --entry "$id" --input "$driver_input" --format "$driver_format"
        --threshold "$threshold" --max-dim "$max_dim" --modulus "$modulus")
    driver_cmd=("${driver_base[@]}" --reps "$driver_reps" --mode "$driver_modes"
        --diagram-out "$reference")

    {
        echo "== id=$id stratum=$stratum set=$SET headline=$headline competitor=$competitor"
        echo "source: $source_kind n=$n tau=$tau max_dim=$max_dim modulus=$modulus seed=$seed collapse=$collapse"
        echo "graph: $graph"
        if [[ "$collapse" == yes ]]; then
            echo "collapse: $collapse_note"
        fi
        echo "driver cmd: $(record_cmd "${driver_cmd[@]}")"
    } >>"$RESULTS"

    if ! run_stats="$(measure_err "$rows" "$errf" "${driver_cmd[@]}")"; then
        ANY_MISMATCH=1
        {
            echo "VOID: the driver exited nonzero; no timing is recorded for this entry"
            sed 's/^/  /' "$errf"
            echo
        } >>"$RESULTS"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$stratum" "$headline" "$competitor" "$n" "$edges_in" "void" >>"$ENTRY_META"
        echo "id=$id VOID (driver exited nonzero; see $(basename "$errf"))" >&2
        continue
    fi

    entry_line="$(row "$rows" kind=entry)"
    graph_edges="$(field graph_edges "$entry_line")"
    balanced="$(field balanced "$entry_line")"
    bars="$(field bars "$(row "$rows" kind=diagram)")"

    # Phase medians, the diagnostic table. Only the working tree's driver
    # produces them; the historical arms have no driver of their own.
    for mode in ${driver_modes//,/ }; do
        for phase in parse distance graph reduce total; do
            value="$(field median_s "$(row "$rows" kind=phase config="$mode" phase="$phase")")"
            if [[ -n "$value" ]]; then
                printf '%s\t%s\t%s\t%s\n' "$id" "$mode" "$phase" "$value" >>"$PHASES"
            fi
        done
        probe_out="$DATA/engine_${SET}_${id}_rss_$mode.rows"
        if probe_stats="$(measure "$probe_out" "${driver_base[@]}" --reps 1 --mode "$mode")"; then
            probe_kb="$(rss_kb "$probe_stats")"
            printf '%s\tdriver\t%s\t%s\t%s\t%s\n' "$id" "$mode" "n/a" "n/a" "$(or_na "$probe_kb")" >>"$TOTALS"
        fi
    done

    entry_void=0
    all_configs=""
    for i in "${!ARM_LABEL[@]}"; do
        for cfg in $(entry_configs "$i" "$file_kind"); do
            case " $all_configs " in
                *" $cfg "*) ;;
                *) all_configs="$all_configs $cfg" ;;
            esac
        done
    done

    # One ripser run per distinct input file. The routing configurations
    # read one file, and ripser has no routing to force, so timing it once
    # per file is the whole external arm. On a cloud entry sparse-file
    # names the other file, so ripser runs twice there.
    declare -A RIPSER_MEDIAN=()
    declare -A RIPSER_DONE=()
    for cfg in $all_configs; do
        if [[ "$competitor" != ripser ]]; then
            continue
        fi
        config_input "$cfg"
        if [[ -n "${RIPSER_DONE[$cfg_file]:-}" ]]; then
            reuse="${RIPSER_DONE[$cfg_file]}"
            RIPSER_MEDIAN[$cfg]="$(field median_s "$reuse")"
            printf '%s\tripser\t%s\t%s\t%s\t%s\n' "$id" "$cfg" \
                "$(field median_s "$reuse")" "$(field iqr_s "$reuse")" \
                "$(or_na "$(rss_kb "$reuse")")" >>"$TOTALS"
            continue
        fi
        ripser_out="$DATA/engine_${SET}_${id}_ripser_$cfg.out"
        ripser_err="$DATA/engine_${SET}_${id}_ripser_$cfg.err"
        # ripser takes --modulus only when it was built with coefficients,
        # so an odd modulus carries competitor = "none" instead.
        ripser_cmd=("$RIPSER_BIN" --format "$cfg_format" --dim "$max_dim"
            --threshold "$threshold" "$cfg_file")
        echo "ripser cmd ($cfg): $(record_cmd "${ripser_cmd[@]}")" >>"$RESULTS"
        if ! ripser_stats="$(measure_repeat "$ripser_out" "$ripser_err" "$REPS" "${ripser_cmd[@]}")"; then
            entry_void=1
            {
                echo "VOID: ripser exited nonzero on the $cfg configuration"
                sed 's/^/  /' "$ripser_err"
            } >>"$RESULTS"
            continue
        fi
        if [[ "$(compare_diagrams "$reference" "$ripser_out")" != yes ]]; then
            entry_void=1
            echo "VOID: ripser's $cfg diagram disagrees with the reference" >>"$RESULTS"
            continue
        fi
        RIPSER_MEDIAN[$cfg]="$(field median_s "$ripser_stats")"
        RIPSER_DONE[$cfg_file]="$ripser_stats"
        printf '%s\tripser\t%s\t%s\t%s\t%s\n' "$id" "$cfg" \
            "$(field median_s "$ripser_stats")" "$(field iqr_s "$ripser_stats")" \
            "$(or_na "$(rss_kb "$ripser_stats")")" >>"$TOTALS"
    done

    # One run per distinct arm command. Two configurations of one arm can
    # name the same file and the same engine: on an entry whose primary
    # file is the triplet file, sparse-file is auto again. Both rows then
    # come from the one timing.
    for i in "${!ARM_LABEL[@]}"; do
        label="${ARM_LABEL[$i]}"
        bin="${ARM_BIN[$i]}"
        declare -A ARM_DONE=()
        for cfg in $(entry_configs "$i" "$file_kind"); do
            config_input "$cfg"
            arm_out="$DATA/engine_${SET}_${id}_${label}_$cfg.out"
            arm_err="$DATA/engine_${SET}_${id}_${label}_$cfg.err"
            arm_cmd=("$bin" --format "$cfg_format" --dim "$max_dim" --threshold "$threshold"
                --modulus "$modulus" --output ripser "${engine_flag[@]+"${engine_flag[@]}"}" "$cfg_file")
            arm_key="$cfg_file|$cfg_engine"
            if [[ -n "${ARM_DONE[$arm_key]:-}" ]]; then
                reuse="${ARM_DONE[$arm_key]}"
                echo "arm cmd ($label, $cfg): $(record_cmd "${arm_cmd[@]}") [same command as an earlier configuration; that timing is reused]" >>"$RESULTS"
                printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$label" "$cfg" \
                    "$(field median_s "$reuse")" "$(field iqr_s "$reuse")" \
                    "$(or_na "$(rss_kb "$reuse")")" >>"$TOTALS"
                echo "id=$id arm=$label cfg=$cfg total=$(field median_s "$reuse") over_ripser=$(ratio "$(field median_s "$reuse")" "${RIPSER_MEDIAN[$cfg]:-}") (reused)" >&2
                continue
            fi
            echo "arm cmd ($label, $cfg): $(record_cmd "${arm_cmd[@]}")" >>"$RESULTS"
            if ! arm_stats="$(measure_repeat "$arm_out" "$arm_err" "$REPS" "${arm_cmd[@]}")"; then
                entry_void=1
                {
                    echo "VOID: arm $label exited nonzero on the $cfg configuration"
                    sed 's/^/  /' "$arm_err"
                } >>"$RESULTS"
                continue
            fi
            if [[ "$(compare_diagrams "$reference" "$arm_out")" != yes ]]; then
                entry_void=1
                echo "VOID: arm $label, configuration $cfg, disagrees with the reference diagram" >>"$RESULTS"
                continue
            fi
            ARM_DONE[$arm_key]="$arm_stats"
            printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$label" "$cfg" \
                "$(field median_s "$arm_stats")" "$(field iqr_s "$arm_stats")" \
                "$(or_na "$(rss_kb "$arm_stats")")" >>"$TOTALS"
            echo "id=$id arm=$label cfg=$cfg total=$(field median_s "$arm_stats") over_ripser=$(ratio "$(field median_s "$arm_stats")" "${RIPSER_MEDIAN[$cfg]:-}")" >&2
        done
    done

    {
        cat "$rows"
        echo "driver run $run_stats"
        echo "AGREEMENT reference_bars=$(or_na "$bars") balanced=$(or_na "$balanced") tolerance=$TOLERANCE void=$entry_void"
        echo "GRAPH edges_at_threshold=$edges_in driver_graph_edges=$(or_na "$graph_edges")"
        echo
    } >>"$RESULTS"

    if ((entry_void != 0)); then
        ANY_MISMATCH=1
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$stratum" "$headline" "$competitor" "$n" "$edges_in" "void" >>"$ENTRY_META"
        echo "id=$id VOID (an arm or ripser lost its comparison)" >&2
    else
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "$stratum" "$headline" "$competitor" "$n" "$edges_in" "ok" >>"$ENTRY_META"
    fi
done <"$ENTRY_FILE"

if ((ENTRIES == 0)); then
    echo "error: filter ONLY=$ONLY matched no entry in the $SET set" >&2
    exit 1
fi

ARMS_TEXT=""
for i in "${!ARM_LABEL[@]}"; do
    ARMS_TEXT="$ARMS_TEXT${ARMS_TEXT:+ }${ARM_LABEL[$i]}:${ARM_COMMIT[$i]}:${ARM_SHA[$i]}:${ARM_CONFIGS[$i]}"
done

{
    echo "<!-- Generated by benchmarks/engine_bench.sh. Do not edit; rerun the script. -->"
    echo
    echo "# Engineering benchmark, serial reduction against ripser, $SET set"
    echo
    echo "**Engineering run, not a registered study.** These numbers tune the"
    echo "engine and decide whether a change lands. No public claim may cite them."
    echo
    if ((RERUN != 0)); then
        echo "**LANDING-RERUN.** ALLOW_RERUN=1 overwrote an earlier landing record, so"
        echo "this set is no longer untouched. Read it as tuning data."
        echo
    fi
    emit_provenance_md
    for i in "${!ARM_LABEL[@]}"; do
        echo "- arm \`${ARM_LABEL[$i]}\`: commit \`${ARM_COMMIT[$i]}\` sha256 \`${ARM_SHA[$i]}\`, version ${ARM_VERSION[$i]}, configurations ${ARM_CONFIGS[$i]}"
    done
    echo "- ripser binary: \`$RIPSER_BIN_DISPLAY\` sha256 \`$RIPSER_SHA\`"
    echo "- ripser build: \`$RIPSER_FLAGS\`"
    echo "- corpus: \`$(basename "$CORPUS")\` version $CORPUS_VERSION dated $CORPUS_DATE, $SET set, $ENTRIES entries"
    echo "- primary total: one fresh process per run, timed by \`measure.py\`; 1 warm-up + $REPS timed repetitions per arm and configuration, and the same for ripser"
    echo "- diagnostic: \`crates/engine-bench\`, one process per entry, one clock per phase, at least $DRIVER_REPS timed repetitions rounded up to keep the rotation balanced"
    echo "- configurations: $ROUTING_STATE. \`routing\` means \`--engine auto|dense|sparse\` on one file, plus \`sparse-file\`, where holos reads the triplet file with \`--format sparse --engine auto\` and ripser reads it with \`--format sparse\`; \`files\` means the dense and the sparse input file"
    echo "- agreement: every arm and every ripser run against the driver's reference diagram, as interval multisets within $TOLERANCE"
    echo "- entry filter: \`$ONLY\`"
    echo "- configuration filter: \`$ONLY_CONFIGS\`"
    echo
    python3 "$HERE/engine_tables.py" "$ENTRY_META" "$TOTALS" "$PHASES" "$ARMS_TEXT"
} >"$RESULTS_MD"

{
    echo "SUMMARY set=$SET arms=$ARMS_TEXT entries=$ENTRIES"
    python3 "$HERE/engine_tables.py" --text "$ENTRY_META" "$TOTALS" "$PHASES" "$ARMS_TEXT"
} >>"$RESULTS"

echo "Results written to $RESULTS and $RESULTS_MD." >&2
echo "Do not copy numbers into documents by hand; rerun this script instead." >&2

if ((ANY_MISMATCH != 0)); then
    echo "FAILURE: at least one entry lost its diagram comparison or its external" >&2
    echo "         arm. The timings of that entry do not count." >&2
    exit 1
fi
