#!/usr/bin/env bash
# Registered end-to-end study for version 3 adaptive collapse.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: collapse_adaptive.sh [--confirm] [-h]

Run the frozen adaptive collapse screen. Pass --confirm for the held-out set.
The confirmation run requires a complete screen manifest for the same corpus.

Environment:
  CARGO           cargo invocation, default "cargo +1.92"
  CORPUS          corpus path, default collapse_adaptive_corpus.toml
  ONLY            glob over entry ids, default all
  ALLOW_DIRTY     set to 1 to run from a dirty tree and mark the record void
  ALLOW_NO_SCREEN set to 1 to bypass the confirmation lock and mark it void
EOF
}

SET=screen
for argument in "$@"; do
    case "$argument" in
        --confirm) SET=confirm ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $argument" >&2
            usage >&2
            exit 2
            ;;
    esac
done

CARGO="${CARGO:-cargo +1.92}"
# shellcheck source=benchmarks/_common.sh
source "$(dirname "$0")/_common.sh"
require_proc

CORPUS="${CORPUS:-$HERE/collapse_adaptive_corpus.toml}"
ONLY="${ONLY:-*}"
BUILD_CPUS="${BUILD_CPUS:-16-31}"
SCREEN_RESULTS="$HERE/results_collapse_adaptive_screen.txt"
MANIFEST="$HERE/results_collapse_adaptive_manifest.txt"
RESULTS="$HERE/results_collapse_adaptive_${SET}.txt"
RESULTS_MD="$HERE/results_collapse_adaptive_${SET}.md"

if [[ ! -r "$CORPUS" ]]; then
    echo "error: no corpus at $(basename "$CORPUS")" >&2
    exit 1
fi
if ! python3 -c 'import tomllib' 2>/dev/null; then
    echo "error: reading the corpus needs Python 3.11 or newer" >&2
    exit 1
fi

read_meta() {
    python3 - "$CORPUS" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as source:
    meta = tomllib.load(source)["meta"]
fields = ("version", "date", "reps", "threads", "budget_per_input_edge", "cpus")
print("\t".join(str(meta[name]) for name in fields))
PY
}

entries() {
    python3 - "$CORPUS" "$1" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as source:
    corpus = tomllib.load(source)
fields = ("id", "family", "n", "coord_dim", "max_dim", "tau", "modulus", "seed")
seen = set()
for entry in corpus.get(sys.argv[2], []):
    missing = [field for field in fields if field not in entry]
    if missing:
        raise SystemExit(f"entry {entry.get('id', '?')} lacks {', '.join(missing)}")
    if entry["id"] in seen:
        raise SystemExit(f"duplicate entry id {entry['id']}")
    seen.add(entry["id"])
    print("\t".join(str(entry[field]) for field in fields))
PY
}

entry_ids() {
    entries "$1" | cut -f1
}

physical_cores() {
    lscpu -p=CPU,CORE,SOCKET 2>/dev/null | awk -F, -v allowed="$1" '
        BEGIN {
            n = split(allowed, parts, ",")
            for (i = 1; i <= n; i++) {
                if (split(parts[i], range, "-") == 2) {
                    for (cpu = range[1]; cpu <= range[2]; cpu++) keep[cpu] = 1
                } else keep[parts[i]] = 1
            }
        }
        /^[0-9]/ && ($1 in keep) { cores[$3 "," $2] = 1 }
        END { for (core in cores) count++; print count + 0 }
    '
}

cpu_set_contains() {
    awk -v allowed="$1" -v wanted="$2" 'BEGIN {
        n = split(allowed, parts, ",")
        for (i = 1; i <= n; i++) {
            if (split(parts[i], range, "-") == 2) {
                if (wanted >= range[1] && wanted <= range[2]) found = 1
            } else if (wanted == parts[i]) found = 1
        }
        print found ? "yes" : "no"
    }'
}

require_screen() {
    if [[ "${ALLOW_NO_SCREEN:-}" == 1 ]]; then
        BYPASS=1
        return
    fi
    if [[ ! -s "$SCREEN_RESULTS" || ! -s "$MANIFEST" ]]; then
        echo "error: confirmation needs a complete adaptive screen and manifest" >&2
        exit 1
    fi
    local expected_sha expected_ids
    expected_sha="$(sha256 "$CORPUS")"
    if ! grep -qx "corpus_sha256=$expected_sha" "$MANIFEST"; then
        echo "error: the adaptive corpus changed after the screen" >&2
        exit 1
    fi
    if ! grep -qx 'filter=\*' "$MANIFEST"; then
        echo "error: a filtered screen cannot unlock confirmation" >&2
        exit 1
    fi
    if ! grep -qx "screen_commit=$(head_commit)" "$MANIFEST"; then
        echo "error: confirmation must use the screen commit" >&2
        exit 1
    fi
    expected_ids="$(entry_ids screen | sort)"
    if [[ "$(sed -n 's/^entry=//p' "$MANIFEST" | sort)" != "$expected_ids" ]]; then
        echo "error: the screen manifest does not cover every registered entry" >&2
        exit 1
    fi
}

IFS=$'\t' read -r CORPUS_VERSION CORPUS_DATE REPS THREADS BUDGET_FACTOR CPUS < <(read_meta)
if ((REPS < 5 || REPS % 5 != 0)); then
    echo "error: corpus repetitions must be at least 5 and divisible by 5" >&2
    exit 1
fi
if [[ "$(cpu_set_contains "$CPUS" 4)" == yes ]]; then
    echo "error: the timing CPU set contains the excluded logical CPU 4" >&2
    exit 1
fi
if [[ "$(physical_cores "$CPUS")" != "$THREADS" ]]; then
    echo "error: $CPUS does not provide the registered $THREADS physical cores" >&2
    exit 1
fi

BYPASS=0
if [[ "$SET" == confirm ]]; then
    require_screen
elif [[ -e "$MANIFEST" ]]; then
    rm -f -- "$MANIFEST"
fi

read -r -a CARGO_PARTS <<<"$CARGO"
# These variables configure the provenance function from _common.sh.
export BUILD_CMD_DISPLAY="$CARGO build --release --locked -p collapse-bench --bin collapse-adaptive-bench"
taskset -c "$BUILD_CPUS" "${CARGO_PARTS[@]}" build --release --locked \
    -p collapse-bench --bin collapse-adaptive-bench
DRIVER="$ROOT/target/release/collapse-adaptive-bench"
export HOLOS_BIN="$DRIVER"
export HOLOS_BIN_DISPLAY="target/release/collapse-adaptive-bench"
export PROV_AFFINITY_OVERRIDE="$CPUS"
emit_provenance "$RESULTS" "adaptive collapse $SET record"
{
    echo "corpus: $(basename "$CORPUS")"
    echo "corpus version: $CORPUS_VERSION"
    echo "corpus date: $CORPUS_DATE"
    echo "corpus sha256: $(sha256 "$CORPUS")"
    echo "set: $SET"
    echo "filter: $ONLY"
    echo "screen protocol bypassed: $BYPASS"
    echo "repetitions: $REPS"
    echo "timing cpus: $CPUS"
    echo "build cpus: $BUILD_CPUS"
    echo
} >>"$RESULTS"

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT
DONE="$WORK/done"
: >"$DONE"

graph_meta() {
    python3 - "$1" "$2" <<'PY'
import math
import sys

points = []
with open(sys.argv[1]) as source:
    for line in source:
        line = line.strip()
        if line and not line.startswith("#"):
            points.append([float(value) for value in line.replace(",", " ").split()])
radius = min(max(math.dist(left, right) for right in points) for left in points)
threshold = radius * float(sys.argv[2])
edges = sum(
    math.dist(points[i], points[j]) <= threshold
    for i in range(len(points))
    for j in range(i + 1, len(points))
)
print(f"{threshold!r}\t{edges}")
PY
}

validate_run() {
    python3 - "$1" "$2" "$3" <<'PY'
import sys

path, scope, limit = sys.argv[1:]
records = []
with open(path) as source:
    for line in source:
        if line.startswith("kind="):
            records.append(dict(field.split("=", 1) for field in line.split()))
agreements = [record for record in records if record.get("kind") == "agreement"]
if len(agreements) != 1 or agreements[0].get("result") != "pass":
    raise SystemExit(f"{path}: exact diagram gate did not pass")
counters = [record for record in records if record.get("kind") == "counters"]
if len(counters) != 5:
    raise SystemExit(f"{path}: expected five counter records")
for record in counters:
    if record.get("stable") != "yes":
        raise SystemExit(f"{path}: unstable counters for {record.get('config')}")
    config = record["config"]
    if config != "none" and int(record.get("artifact_bytes", "0")) == 0:
        raise SystemExit(f"{path}: empty artifact for {config}")
    if config.startswith("v3-"):
        completeness = record.get("completeness")
        if scope == "complete" and completeness != "complete":
            raise SystemExit(f"{path}: complete arm stopped early")
        if scope == "budget" and int(record["work_used"]) > int(limit):
            raise SystemExit(f"{path}: budget arm exceeded its work limit")
phases = [record for record in records if record.get("kind") == "phase"]
if len(phases) != 40:
    raise SystemExit(f"{path}: expected forty phase summaries")
PY
}

CONFIGS=(none v1 v2 v3-h1 v3-h2)
ENTRIES=0
while IFS=$'\t' read -r id family n coord_dim max_dim tau modulus seed; do
    # ONLY is a documented glob over entry ids.
    # shellcheck disable=SC2053
    [[ "$id" == $ONLY ]] || continue
    ENTRIES=$((ENTRIES + 1))
    cloud="$DATA/adaptive_${SET}_${id}.csv"
    python3 "$HERE/gen_cloud.py" "$n" "$coord_dim" "$seed" "$family" >"$cloud"
    IFS=$'\t' read -r threshold input_edges < <(graph_meta "$cloud" "$tau")
    budget=$((input_edges * BUDGET_FACTOR))
    complete="$WORK/${id}.complete"
    budgeted="$WORK/${id}.budget"

    echo "running $id complete" >&2
    taskset -c "$CPUS" "$DRIVER" --input "$cloud" --entry "${id}-complete" \
        --threshold "$threshold" --max-dim "$max_dim" --modulus "$modulus" \
        --threads "$THREADS" --reps "$REPS" >"$complete"
    validate_run "$complete" complete 0

    echo "running $id budget $budget" >&2
    taskset -c "$CPUS" "$DRIVER" --input "$cloud" --entry "${id}-budget" \
        --threshold "$threshold" --max-dim "$max_dim" --modulus "$modulus" \
        --threads "$THREADS" --reps "$REPS" --work-limit "$budget" >"$budgeted"
    validate_run "$budgeted" budget "$budget"

    {
        echo "kind=corpus_entry entry=$id set=$SET family=$family n=$n coord_dim=$coord_dim max_dim=$max_dim tau=$tau threshold=$threshold modulus=$modulus seed=$seed input_sha256=$(sha256 "$cloud") budget=$budget"
        cat "$complete"
        cat "$budgeted"
    } >>"$RESULTS"

    for scope in complete budget; do
        for config in "${CONFIGS[@]}"; do
            if [[ "$scope" == budget && "$config" != v3-* ]]; then
                continue
            fi
            probe_out="$WORK/${id}.${scope}.${config}.out"
            probe_err="$WORK/${id}.${scope}.${config}.err"
            command=("$DRIVER" --input "$cloud" \
                --entry "${id}-memory-${scope}-${config}" --threshold "$threshold" \
                --max-dim "$max_dim" --modulus "$modulus" --threads "$THREADS" \
                --reps 1 --configs "$config")
            if [[ "$scope" == budget ]]; then
                command+=(--work-limit "$budget")
            fi
            memory="$(MEASURE_AFFINITY="$CPUS" measure_err \
                "$probe_out" "$probe_err" "${command[@]}")"
            if [[ "$(field max_rss_kb "$memory")" == 0 ]]; then
                echo "error: memory probe returned no reading for $id $scope $config" >&2
                exit 1
            fi
            echo "kind=arm_memory entry=$id scope=$scope config=$config $memory" >>"$RESULTS"
        done
    done
    echo "entry=$id" >>"$DONE"
done < <(entries "$SET")

if ((ENTRIES == 0)); then
    echo "error: filter ONLY=$ONLY matched no $SET entry" >&2
    exit 1
fi

python3 - "$RESULTS" "$RESULTS_MD" "$SET" "$BYPASS" <<'PY'
import sys

source_path, output_path, study_set, bypass = sys.argv[1:]
records = []
with open(source_path) as source:
    for line in source:
        if line.startswith("kind="):
            records.append(dict(field.split("=", 1) for field in line.split()))

entries = {record["entry"]: record for record in records if record["kind"] == "corpus_entry"}
driver_entries = {record["entry"]: record for record in records if record["kind"] == "entry"}
phases = {
    (record["entry"], record["config"], record["phase"]): record
    for record in records if record["kind"] == "phase"
}
counters = {
    (record["entry"], record["config"]): record
    for record in records if record["kind"] == "counters"
}
memory = {
    (record["entry"], record["scope"], record["config"]): record
    for record in records if record["kind"] == "arm_memory"
}

def seconds(entry, config, phase, field="median_s"):
    return float(phases[(entry, config, phase)][field])

def count(entry, config, field):
    if config == "none":
        source = driver_entries[entry]
        mapping = {
            "output_edges": "input_edges",
            "output_triangles": "input_triangles",
            "output_tetrahedra": "input_tetrahedra",
        }
        return int(source[mapping[field]])
    return int(counters[(entry, config)][field])

with open(output_path, "w") as output:
    output.write("<!-- Generated by benchmarks/collapse_adaptive.sh. -->\n\n")
    output.write(f"# Adaptive collapse, {study_set} set\n\n")
    if bypass == "1":
        output.write("**SCREEN-PROTOCOL-BYPASSED. This record is void.**\n\n")
    output.write("Times are seconds. Each value is a median over the registered repetitions.\n")
    output.write("The raw record also contains IQRs, maxima, isolated peak RSS, artifact sizes, and work counters.\n\n")
    output.write("| entry | scope | config | edges out | triangles out | tetrahedra out | collapse | reduce | certified | target result | product result |\n")
    output.write("|:--|:--|:--|--:|--:|--:|--:|--:|--:|:--|:--|\n")
    for base in entries:
        complete_v1 = f"{base}-complete"
        v1_certified = seconds(complete_v1, "v1", "certified")
        v1_triangles = count(complete_v1, "v1", "output_triangles")
        v2_triangles = count(complete_v1, "v2", "output_triangles")
        v1_tetrahedra = count(complete_v1, "v1", "output_tetrahedra")
        v2_tetrahedra = count(complete_v1, "v2", "output_tetrahedra")
        for scope in ("complete", "budget"):
            entry = f"{base}-{scope}"
            configs = ("none", "v1", "v2", "v3-h1", "v3-h2") if scope == "complete" else ("v3-h1", "v3-h2")
            for config in configs:
                edges = count(entry, config, "output_edges")
                triangles = count(entry, config, "output_triangles")
                tetrahedra = count(entry, config, "output_tetrahedra")
                certified = seconds(entry, config, "certified")
                target = "context"
                product = "context"
                if config == "v3-h1":
                    if scope == "complete":
                        target = "yes" if triangles <= min(v1_triangles, v2_triangles) else "no"
                    else:
                        input_triangles = count(entry, "none", "output_triangles")
                        target = "yes" if triangles * 2 <= input_triangles else "no"
                    product = "yes" if certified <= v1_certified else "no"
                elif config == "v3-h2":
                    if scope == "complete":
                        target = "yes" if tetrahedra <= min(v1_tetrahedra, v2_tetrahedra) else "no"
                    else:
                        input_tetrahedra = count(entry, "none", "output_tetrahedra")
                        target = "yes" if tetrahedra * 2 <= input_tetrahedra else "no"
                    product = "yes" if certified <= v1_certified else "no"
                output.write(
                    f"| {base} | {scope} | {config} | {edges} | {triangles} | {tetrahedra} | "
                    f"{seconds(entry, config, 'collapse'):.6f} | {seconds(entry, config, 'reduce'):.6f} | "
                    f"{certified:.6f} | {target} | {product} |\n"
                )
PY

if [[ "$SET" == screen && "$ONLY" == "*" ]]; then
    {
        echo "corpus_file=$(basename "$CORPUS")"
        echo "corpus_version=$CORPUS_VERSION"
        echo "corpus_date=$CORPUS_DATE"
        echo "corpus_sha256=$(sha256 "$CORPUS")"
        echo "screen_commit=$(head_commit)"
        echo "filter=$ONLY"
        cat "$DONE"
    } >"$MANIFEST"
fi

echo "Results written to $RESULTS and $RESULTS_MD." >&2
