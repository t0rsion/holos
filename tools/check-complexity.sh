#!/usr/bin/env bash
set -euo pipefail

readonly EXPECTED_VERSION="rust-code-analysis-cli 0.0.25"

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

report=$(
    {
        rust-code-analysis-cli -p crates -m -O json -j 8
        rust-code-analysis-cli -p benchmarks -m -O json -j 8
    } | jq -rs '
        [
            .[] as $file
            | $file
            | ..
            | objects
            | select(.kind == "function")
            | {
                file: $file.name,
                name,
                line: .start_line,
                complexity: (.metrics.cyclomatic.max | floor)
            }
        ] as $functions
        | {
            tool: "rust-code-analysis-cli 0.0.25",
            thresholds: {fine: "1-5", watch: "6-10", violation: "11+"},
            functions: ($functions | length),
            fine: ([$functions[] | select(.complexity <= 5)] | length),
            watch: ([$functions[] | select(
                .complexity >= 6 and .complexity <= 10
            )] | length),
            violations: ([$functions[] | select(.complexity >= 11)]
                | sort_by(-.complexity, .file, .line))
        }
    '
)

printf '%s\n' "$report"
violations=$(jq '.violations | length' <<<"$report")
if ((violations > 0)); then
    echo "check-complexity: $violations function(s) have complexity 11 or higher" >&2
    exit 1
fi
