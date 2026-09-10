use std::collections::BTreeMap;

use crate::ProofError;

use super::linear::{Vector, nullspace, rref};
use super::{MapTerm, Space};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContinuationKind {
    Unique,
    Ambiguous,
    NoExtension,
    NoNonzeroContinuation,
}

pub(crate) struct Continuation {
    pub(crate) kind: ContinuationKind,
    pub(crate) target: Vec<MapTerm>,
    pub(crate) ambiguity: Vec<Vec<MapTerm>>,
}

pub(crate) fn continuation(
    old: &Space,
    new: &Space,
    common: &Space,
    selected: &[MapTerm],
    modulus: u32,
) -> Result<Continuation, ProofError> {
    let (old_columns, _) = old.restriction_to(common, modulus)?;
    let (new_columns, _) = new.restriction_to(common, modulus)?;
    let relation_equations = continuation_relations(
        &old_columns,
        &new_columns,
        old.rank(),
        common.rank(),
        modulus,
    );
    let relation = nullspace(
        relation_equations,
        old.rank() + new.rank(),
        u64::from(modulus),
    );
    let equations = continuation_equations(&relation, old.rank());
    let right = selected_vector(selected, old.rank());
    let Some((particular, kernel)) =
        affine_solution(&equations, &right, relation.len(), u64::from(modulus))
    else {
        return Ok(no_extension());
    };
    let target = relation_target(&particular, &relation, old.rank(), u64::from(modulus));
    let ambiguity = continuation_ambiguity(&kernel, &relation, old.rank(), modulus);
    let kind = continuation_kind(&ambiguity, &target);
    Ok(Continuation {
        kind,
        target: map_terms(&target),
        ambiguity: ambiguity.iter().map(map_terms).collect(),
    })
}

fn continuation_relations(
    old_columns: &[Vec<MapTerm>],
    new_columns: &[Vec<MapTerm>],
    old_rank: usize,
    common_rank: usize,
    modulus: u32,
) -> Vec<Vector> {
    let mut equations = vec![Vector::default(); common_rank];
    for (variable, column) in old_columns.iter().enumerate() {
        for term in column {
            equations[term.target].insert(variable, term.coefficient);
        }
    }
    for (offset, column) in new_columns.iter().enumerate() {
        for term in column {
            equations[term.target].insert(
                old_rank + offset,
                (u64::from(modulus) - u64::from(term.coefficient)) as u32,
            );
        }
    }
    equations
}

fn continuation_equations(relation: &[Vector], old_rank: usize) -> Vec<Vector> {
    let mut equations = vec![Vector::default(); old_rank];
    for (variable, row) in relation.iter().enumerate() {
        for (&position, &coefficient) in row.0.range(..old_rank) {
            equations[position].insert(variable, coefficient);
        }
    }
    equations
}

fn selected_vector(selected: &[MapTerm], old_rank: usize) -> Vec<u32> {
    let selected_map = selected
        .iter()
        .map(|term| (term.target, term.coefficient))
        .collect::<BTreeMap<_, _>>();
    (0..old_rank)
        .map(|position| selected_map.get(&position).copied().unwrap_or(0))
        .collect()
}

fn no_extension() -> Continuation {
    Continuation {
        kind: ContinuationKind::NoExtension,
        target: Vec::new(),
        ambiguity: Vec::new(),
    }
}

fn continuation_ambiguity(
    kernel: &[Vector],
    relation: &[Vector],
    old_rank: usize,
    modulus: u32,
) -> Vec<Vector> {
    rref(
        kernel
            .iter()
            .map(|coefficients| {
                relation_target(coefficients, relation, old_rank, u64::from(modulus))
            })
            .collect(),
        u64::from(modulus),
    )
}

fn continuation_kind(ambiguity: &[Vector], target: &Vector) -> ContinuationKind {
    if !ambiguity.is_empty() {
        ContinuationKind::Ambiguous
    } else if target.is_zero() {
        ContinuationKind::NoNonzeroContinuation
    } else {
        ContinuationKind::Unique
    }
}

fn affine_solution(
    equations: &[Vector],
    right: &[u32],
    variables: usize,
    modulus: u64,
) -> Option<(Vector, Vec<Vector>)> {
    let augmented = equations
        .iter()
        .zip(right)
        .map(|(equation, &value)| {
            let mut row = equation.clone();
            row.insert(variables, value);
            row
        })
        .collect::<Vec<_>>();
    let reduced = rref(augmented, modulus);
    if reduced
        .iter()
        .any(|row| row.leading().is_some_and(|(pivot, _)| pivot == variables))
    {
        return None;
    }
    let mut particular = Vector::default();
    for row in reduced {
        let Some((pivot, _)) = row.leading() else {
            continue;
        };
        if let Some(&value) = row.0.get(&variables) {
            particular.insert(pivot, value);
        }
    }
    Some((
        particular,
        nullspace(equations.to_vec(), variables, modulus),
    ))
}

fn relation_target(
    coefficients: &Vector,
    relation: &[Vector],
    old_rank: usize,
    modulus: u64,
) -> Vector {
    let mut target = Vector::default();
    for (&relation_position, &coefficient) in &coefficients.0 {
        for (&position, &value) in relation[relation_position].0.range(old_rank..) {
            let mut term = Vector::default();
            term.insert(position - old_rank, value);
            target.add_scaled(&term, u64::from(coefficient), modulus);
        }
    }
    target
}

fn map_terms(vector: &Vector) -> Vec<MapTerm> {
    vector
        .0
        .iter()
        .map(|(&target, &coefficient)| MapTerm {
            target,
            coefficient,
        })
        .collect()
}
