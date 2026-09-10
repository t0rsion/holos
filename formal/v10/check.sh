#!/usr/bin/env bash
set -euo pipefail

proof_dir="$(cd "$(dirname "$0")" && pwd)"

unsat_proofs=(
    filtered_support
    factorization_stability
    guard_transitivity
    local_atom_scope
    source_binding
)

for proof in "${unsat_proofs[@]}"; do
    result="$(z3 "$proof_dir/$proof.smt2")"
    if [[ "$result" != "unsat" ]]; then
        echo "$proof: expected unsat, got $result" >&2
        exit 1
    fi
    echo "$proof: checked"
done

result="$(z3 "$proof_dir/strict_containment.smt2")"
if [[ "$result" != "sat" ]]; then
    echo "strict_containment: expected sat, got $result" >&2
    exit 1
fi
echo "strict_containment: checked"
