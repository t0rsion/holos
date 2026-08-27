#!/usr/bin/env bash
set -euo pipefail

proof_dir="$(cd "$(dirname "$0")" && pwd)"

for proof in blocker_necessity branch_partition disjoint_bound; do
    result="$(z3 "$proof_dir/$proof.smt2")"
    if [[ "$result" != "unsat" ]]; then
        echo "$proof: expected unsat, got $result" >&2
        exit 1
    fi
    echo "$proof: checked"
done
