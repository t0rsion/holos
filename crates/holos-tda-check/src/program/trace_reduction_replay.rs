use std::collections::BTreeMap;

use crate::proof::{ProofColumn, SparseColumn};
use crate::{ProofError, inverse_mod};

pub(super) fn valid_prefix_len(
    candidates: &[ProofColumn],
    boundaries: &[SparseColumn],
    modulus: u32,
    term_limit: usize,
) -> Result<usize, ProofError> {
    let modulus = modulus as u64;
    let mut pivots = BTreeMap::new();
    let mut terms = 0usize;
    for (target, transform) in candidates.iter().enumerate() {
        let Some(reduced) = candidate_reduction(target, transform, boundaries, modulus)? else {
            return Ok(target);
        };
        if let Some((pivot, _)) = reduced.pivot() {
            if pivots.insert(pivot, target).is_some() {
                return Ok(target);
            }
        }
        terms = terms
            .checked_add(transform.terms.len())
            .ok_or_else(|| ProofError::new("reduction term count overflows usize"))?;
        if terms > term_limit {
            return Err(ProofError::new("reduction repair exceeds the term limit"));
        }
    }
    Ok(candidates.len())
}

fn candidate_reduction(
    target: usize,
    transform: &ProofColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> Result<Option<SparseColumn>, ProofError> {
    let mut reduced = SparseColumn::default();
    let mut previous = None;
    for term in &transform.terms {
        if term.index > target || previous.is_some_and(|index| index >= term.index) {
            return Ok(None);
        }
        if term.coefficient == 0 || term.coefficient as u64 >= modulus {
            return Err(ProofError::new(
                "reduction repair found an invalid coefficient",
            ));
        }
        let boundary = boundaries
            .get(term.index)
            .ok_or_else(|| ProofError::new("reduction repair found an invalid boundary"))?;
        reduced.add_scaled(boundary, term.coefficient as u64, modulus);
        previous = Some(term.index);
    }
    let unit = transform
        .terms
        .last()
        .is_some_and(|term| (term.index, term.coefficient) == (target, 1));
    Ok(unit.then_some(reduced))
}

pub(super) fn replay_reduction_work(
    prefix: &[ProofColumn],
    boundaries: &[SparseColumn],
    modulus: u32,
    term_limit: usize,
) -> Result<usize, ProofError> {
    let modulus = modulus as u64;
    let mut state = replay_retained_prefix(prefix, boundaries, modulus, term_limit)?;
    replay_suffix(&mut state, boundaries, prefix.len(), modulus, term_limit)?;
    Ok(state.additions)
}

struct ReplayState {
    reduced: Vec<SparseColumn>,
    basis: Vec<SparseColumn>,
    pivot_owner: BTreeMap<usize, usize>,
    additions: usize,
    terms: usize,
}

fn replay_retained_prefix(
    prefix: &[ProofColumn],
    boundaries: &[SparseColumn],
    modulus: u64,
    term_limit: usize,
) -> Result<ReplayState, ProofError> {
    let mut state = ReplayState {
        reduced: Vec::with_capacity(boundaries.len()),
        basis: Vec::with_capacity(boundaries.len()),
        pivot_owner: BTreeMap::new(),
        additions: 0,
        terms: 0,
    };
    for (index, transform) in prefix.iter().enumerate() {
        let column = candidate_reduction(index, transform, boundaries, modulus)?
            .ok_or_else(|| ProofError::new("invalid retained reduction prefix"))?;
        if let Some((pivot, _)) = column.pivot() {
            if state.pivot_owner.insert(pivot, index).is_some() {
                return Err(ProofError::new(
                    "retained reduction prefix has duplicate pivots",
                ));
            }
        }
        let mut basis_column = SparseColumn::default();
        for term in &transform.terms {
            basis_column.insert(term.index, term.coefficient as u64);
        }
        state.terms = state
            .terms
            .checked_add(basis_column.0.len())
            .ok_or_else(|| ProofError::new("reduction term count overflows usize"))?;
        if state.terms > term_limit {
            return Err(ProofError::new("reduction repair exceeds the term limit"));
        }
        state.reduced.push(column);
        state.basis.push(basis_column);
    }
    Ok(state)
}

fn replay_suffix(
    state: &mut ReplayState,
    boundaries: &[SparseColumn],
    prefix_len: usize,
    modulus: u64,
    term_limit: usize,
) -> Result<(), ProofError> {
    for (index, boundary) in boundaries.iter().enumerate().skip(prefix_len) {
        replay_suffix_column(state, index, boundary, modulus, term_limit)?;
    }
    Ok(())
}

fn replay_suffix_column(
    state: &mut ReplayState,
    index: usize,
    boundary: &SparseColumn,
    modulus: u64,
    term_limit: usize,
) -> Result<(), ProofError> {
    let mut column = boundary.clone();
    let mut transform = SparseColumn::default();
    transform.insert(index, 1);
    while let Some((pivot, coefficient)) = column.pivot() {
        let Some(&owner) = state.pivot_owner.get(&pivot) else {
            break;
        };
        let owner_coefficient = state.reduced[owner]
            .pivot()
            .expect("pivot owner has a nonempty column")
            .1;
        let factor =
            (modulus - coefficient * inverse_mod(owner_coefficient, modulus) % modulus) % modulus;
        column.add_scaled(&state.reduced[owner], factor, modulus);
        transform.add_scaled(&state.basis[owner], factor, modulus);
        state.additions = state
            .additions
            .checked_add(1)
            .ok_or_else(|| ProofError::new("column addition count overflows usize"))?;
    }
    if let Some((pivot, _)) = column.pivot() {
        state.pivot_owner.insert(pivot, index);
    }
    state.terms = state
        .terms
        .checked_add(transform.0.len())
        .ok_or_else(|| ProofError::new("reduction term count overflows usize"))?;
    if state.terms > term_limit {
        return Err(ProofError::new("reduction repair exceeds the term limit"));
    }
    state.reduced.push(column);
    state.basis.push(transform);
    Ok(())
}
