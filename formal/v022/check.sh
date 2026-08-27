#!/usr/bin/env bash
set -euo pipefail

proof_dir="$(cd "$(dirname "$0")" && pwd)"

for proof in maximal_failures component_feasibility frontier_composition relative_boundary; do
    result="$(z3 "$proof_dir/$proof.smt2")"
    if [[ "$result" != "unsat" ]]; then
        echo "$proof: expected unsat, got $result" >&2
        exit 1
    fi
    echo "$proof: checked"
done
