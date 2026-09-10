#!/usr/bin/env bash
set -euo pipefail

readonly ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

base_ref="${SOURCE_LOC_BASE:-origin/main}"
if [[ $# -gt 1 ]]; then
    echo "usage: SOURCE_LOC_BASE=ref tools/report-source-loc.sh [ref]" >&2
    exit 2
fi
if [[ $# == 1 ]]; then
    base_ref="$1"
fi
verify_ref="$base_ref^{commit}"
if [[ "$base_ref" == *^ ]]; then
    verify_ref="$base_ref"
fi
if ! git rev-parse --verify "$verify_ref" >/dev/null 2>&1; then
    if [[ -z "$base_ref" || "$base_ref" =~ ^0+$ ]]; then
        base_ref='HEAD^'
    fi
    verify_ref="$base_ref^{commit}"
    if [[ "$base_ref" == *^ ]]; then
        verify_ref="$base_ref"
    fi
    if ! git rev-parse --verify "$verify_ref" >/dev/null 2>&1; then
        echo "report-source-loc: cannot resolve base revision: $base_ref" >&2
        exit 2
    fi
fi

classify_path() {
    local path="$1"
    case "$path" in
        vendor/*|*/vendor/*|third_party/*|*/third_party/*|generated/*|*/generated/*|_generated/*|*/_generated/*)
            printf '%s\n' excluded
            ;;
        tests/*|*/tests/*|test/*|*/test/*|tests.rs|*/tests.rs|test.rs|*/test.rs|*_tests.rs|*_test.py|test_*.py|*/test_*.py)
            printf '%s\n' test
            ;;
        benchmarks/*)
            printf '%s\n' benchmark
            ;;
        tools/*)
            printf '%s\n' tool
            ;;
        *)
            printf '%s\n' production
            ;;
    esac
}

language_for() {
    case "$1" in
        *.rs) printf '%s\n' rust ;;
        *.py) printf '%s\n' python ;;
        *) printf '%s\n' unknown ;;
    esac
}

count_stream() {
    awk '{ lines += 1; if (NF) nonblank += 1 }
        END { printf "%d %d\n", lines + 0, nonblank + 0 }'
}

declare -A paths=()
while IFS= read -r -d '' path; do
    case "$path" in
        *.rs|*.py) paths["$path"]=1 ;;
    esac
done < <(git ls-tree -r -z --name-only "$base_ref")
while IFS= read -r -d '' path; do
    paths["$path"]=1
done < <(git ls-files -z -- '*.rs' '*.py')

declare -A base_files=()
declare -A current_files=()
declare -A base_lines=()
declare -A current_lines=()
declare -A base_nonblank=()
declare -A current_nonblank=()

for path in "${!paths[@]}"; do
    group=$(classify_path "$path")
    language=$(language_for "$path")
    key="$language/$group"

    if git cat-file -e "$base_ref:$path" 2>/dev/null; then
        read -r lines nonblank < <(git show "$base_ref:$path" | count_stream)
        base_files["$key"]=$(( ${base_files["$key"]:-0} + 1 ))
        base_lines["$key"]=$(( ${base_lines["$key"]:-0} + lines ))
        base_nonblank["$key"]=$(( ${base_nonblank["$key"]:-0} + nonblank ))
    fi

    if [[ -f "$path" ]]; then
        read -r lines nonblank < <(count_stream < "$path")
        current_files["$key"]=$(( ${current_files["$key"]:-0} + 1 ))
        current_lines["$key"]=$(( ${current_lines["$key"]:-0} + lines ))
        current_nonblank["$key"]=$(( ${current_nonblank["$key"]:-0} + nonblank ))
    fi
done

printf 'source-loc-report: base=%s (%s), current=working-tree\n' \
    "$base_ref" "$(git rev-parse "$verify_ref")"
printf 'language/category\tbase files\tcurrent files\tdelta files\tbase lines\tcurrent lines\tdelta lines\tbase nonblank\tcurrent nonblank\tdelta nonblank\n'

total_base_files=0
total_current_files=0
total_base_lines=0
total_current_lines=0
total_base_nonblank=0
total_current_nonblank=0
for language in rust python; do
    for group in production test benchmark tool excluded; do
        key="$language/$group"
        bf=${base_files["$key"]:-0}
        cf=${current_files["$key"]:-0}
        bl=${base_lines["$key"]:-0}
        cl=${current_lines["$key"]:-0}
        bn=${base_nonblank["$key"]:-0}
        cn=${current_nonblank["$key"]:-0}
        if ((bf + cf > 0)); then
            printf '%s/%s\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n' \
                "$language" "$group" "$bf" "$cf" "$((cf - bf))" \
                "$bl" "$cl" "$((cl - bl))" "$bn" "$cn" "$((cn - bn))"
        fi
        if [[ "$group" != excluded ]]; then
            total_base_files=$((total_base_files + bf))
            total_current_files=$((total_current_files + cf))
            total_base_lines=$((total_base_lines + bl))
            total_current_lines=$((total_current_lines + cl))
            total_base_nonblank=$((total_base_nonblank + bn))
            total_current_nonblank=$((total_current_nonblank + cn))
        fi
    done
done

printf 'total\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n' \
    "$total_base_files" "$total_current_files" \
    "$((total_current_files - total_base_files))" \
    "$total_base_lines" "$total_current_lines" \
    "$((total_current_lines - total_base_lines))" \
    "$total_base_nonblank" "$total_current_nonblank" \
    "$((total_current_nonblank - total_base_nonblank))"

echo "source-loc-report: physical and nonblank lines are reported for review; no LOC limit is enforced"
