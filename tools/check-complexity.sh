#!/usr/bin/env bash
set -euo pipefail

readonly EXPECTED_VERSION="rust-code-analysis-cli 0.0.25"
readonly ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v rust-code-analysis-cli >/dev/null 2>&1; then
    echo "check-complexity: install rust-code-analysis-cli 0.0.25" >&2
    exit 2
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "check-complexity: install jq" >&2
    exit 2
fi

actual_version=$(rust-code-analysis-cli --version)
if [[ "$actual_version" != "$EXPECTED_VERSION" ]]; then
    echo "check-complexity: expected $EXPECTED_VERSION, found $actual_version" >&2
    exit 2
fi

analysis_jobs="${COMPLEXITY_JOBS:-${RUST_CODE_ANALYSIS_JOBS:-2}}"
if [[ ! "$analysis_jobs" =~ ^[1-9][0-9]*$ ]]; then
    echo "check-complexity: COMPLEXITY_JOBS must be a positive integer" >&2
    exit 2
fi

tracked_sources=$(
    git ls-files -- '*.rs' '*.py' |
        jq -Rsc 'split("\n") | map(select(length > 0))'
)

declare -A source_roots=()
while IFS= read -r path; do
    case "$path" in
        */*) source_roots["${path%%/*}"]=1 ;;
        *) source_roots["$path"]=1 ;;
    esac
done < <(
    git ls-files -- '*.rs' '*.py' |
        while IFS= read -r path; do
            if [[ "$path" =~ (^|/)(vendor|third_party|generated|_generated)(/|$) ]]; then
                continue
            fi
            printf '%s\n' "$path"
        done
)
mapfile -t analysis_paths < <(printf '%s\n' "${!source_roots[@]}" | sort)

base_ref="${COMPLEXITY_BASE:-${SOURCE_REVIEW_BASE:-}}"
changed_files='[]'
resolved_base=''
verify_ref="$base_ref^{commit}"
if [[ "$base_ref" == *^ ]]; then
    verify_ref="$base_ref"
fi
if [[ -n "$base_ref" ]] && git rev-parse --verify "$verify_ref" >/dev/null 2>&1; then
    resolved_base="$base_ref"
    changed_files=$(
        git diff --name-only "$base_ref" -- '*.rs' '*.py' |
            jq -Rsc 'split("\n") | map(select(length > 0))'
    )
fi

report=$(
    {
        for analysis_path in "${analysis_paths[@]}"; do
            rust-code-analysis-cli -p "$analysis_path" -m -O json -j "$analysis_jobs"
        done
    } | jq -rs --argjson tracked "$tracked_sources" --argjson changed "$changed_files" --arg base "$resolved_base" '
        def normalized_path:
            sub("^\\./"; "");
        def excluded_path:
            test("(^|/)(vendor|third_party|generated|_generated)(/|$)")
            or test("(^|/)(vendor|third_party|generated|_generated)[^/]*\\.(rs|py)$");
        def source_group($path):
            if ($path | test("(^|/)(tests?|test_[^/]*)/")
                or test("(^|/)(tests?|test_[^/]*)\\.(rs|py)$")
                or test("(^|/)[^/]*_tests?\\.(rs|py)$")) then "test"
            elif ($path | startswith("benchmarks/")) then "benchmark"
            elif ($path | startswith("tools/")) then "tool"
            else "production"
            end;
        def language($path):
            if ($path | endswith(".rs")) then "rust" else "python" end;
        [
            .[] as $file
            | ($file.name | normalized_path) as $path
            | select(($tracked | index($path)) != null)
            | select(($path | excluded_path) | not)
            | $file
            | ..
            | objects
            | select(.kind == "function")
            | {
                file: $path,
                language: language($path),
                group: source_group($path),
                name,
                line: .start_line,
                complexity: (.metrics.cyclomatic.max | floor),
                touched_file: (($changed | index($path)) != null)
            }
        ] as $functions
        | [
            .[] as $file
            | ($file.name | normalized_path) as $path
            | select(($tracked | index($path)) != null)
            | select(($path | excluded_path) | not)
            | $path
        ] | unique as $analyzed_files
        | ($functions | map(select(.complexity >= 11)
            | . + {class: (if .complexity > 15 then "split" else "now" end)}
          ) | sort_by(-.complexity, .file, .line)) as $violations
        | {
            tool: "rust-code-analysis-cli 0.0.25",
            thresholds: {
                fine: "1-5",
                watch: "6-10",
                now: "11-15",
                split: ">15"
            },
            base: (if $base == "" then null else $base end),
            files: {
                tracked: ($tracked | map(select((. | excluded_path) | not)) | length),
                excluded: ($tracked | map(select(. | excluded_path)) | length),
                analyzed: ($analyzed_files | length),
                missing: (($tracked | map(select((. | excluded_path) | not)) - $analyzed_files) | sort)
            },
            file_groups: (reduce ($tracked | map(select((. | excluded_path) | not))[]) as $path
                ({production: 0, test: 0, benchmark: 0, tool: 0};
                .[source_group($path)] += 1)),
            functions: ($functions | length),
            fine: ([$functions[] | select(.complexity <= 5)] | length),
            watch: ([$functions[] | select(
                .complexity >= 6 and .complexity <= 10
            )] | sort_by(-.complexity, .file, .line)),
            watch_count: ([$functions[] | select(
                .complexity >= 6 and .complexity <= 10
            )] | length),
            violations: $violations,
            violation_counts: {
                now: ([$violations[] | select(.class == "now")] | length),
                split: ([$violations[] | select(.class == "split")] | length)
            },
            maximum: (if $functions == [] then 0 else ($functions | max_by(.complexity).complexity) end),
            groups: (reduce $functions[] as $function ({};
                .[$function.group] = ((.[$function.group] // 0) + 1)))
        }
    '
)

printf '%s\n' "$report"
violations=$(jq '.violations | length' <<<"$report")
missing=$(jq '.files.missing | length' <<<"$report")
if ((missing > 0)); then
    echo "check-complexity: $missing tracked source file(s) were not analyzed" >&2
    jq -r '.files.missing[]' <<<"$report" >&2
    exit 2
fi
if ((violations > 0)); then
    echo "check-complexity: $violations function(s) have complexity 11 or higher" >&2
    exit 1
fi
