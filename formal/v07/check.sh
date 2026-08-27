#!/usr/bin/env bash
set -euo pipefail

proof_dir="$(cd "$(dirname "$0")" && pwd)"

proofs=(
    blocker_necessity
    branch_partition
    disjoint_bound
    component_feasibility
    frontier_composition
    maximal_failures
    relative_boundary
    portfolio_argmin
    reduction_factorization
    unique_pivot_pairing
    radius_graph_completeness
)

for proof in "${proofs[@]}"; do
    result="$(z3 "$proof_dir/$proof.smt2")"
    if [[ "$result" != "unsat" ]]; then
        echo "$proof: expected unsat, got $result" >&2
        exit 1
    fi
    echo "$proof: checked"
done
