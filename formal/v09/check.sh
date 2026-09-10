#!/usr/bin/env bash
set -euo pipefail

proof_dir="$(cd "$(dirname "$0")" && pwd)"

proofs=(
    antichain_upward_closure
    degree_rips_monotonicity
    restriction_square
    affine_fiber_trichotomy
    generalized_rank_bounds
    circular_family_gating
)

for proof in "${proofs[@]}"; do
    result="$(z3 "$proof_dir/$proof.smt2")"
    if [[ "$result" != "unsat" ]]; then
        echo "$proof: expected unsat, got $result" >&2
        exit 1
    fi
    echo "$proof: checked"
done
