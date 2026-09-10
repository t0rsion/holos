use std::collections::{BTreeMap, BTreeSet};

use super::super::model::{ChangeColumn, FiltrationSimplex, ReductionGuard, ReductionGuardKind};
use super::super::reduction::SparseColumn;

pub(in crate::certificate) fn add_change_guards(
    guards: &mut BTreeSet<ReductionGuard>,
    columns: &[ChangeColumn],
    simplices: &[FiltrationSimplex],
) {
    for (target, column) in columns.iter().enumerate() {
        for term in &column.terms {
            if term.index != target {
                guards.insert(ReductionGuard {
                    kind: ReductionGuardKind::ChangeOfBasis,
                    earlier: simplices[term.index].clone(),
                    later: simplices[target].clone(),
                });
            }
        }
    }
}

pub(in crate::certificate) fn add_pivot_guards(
    guards: &mut BTreeSet<ReductionGuard>,
    reduced: &[SparseColumn],
    row_simplices: &[FiltrationSimplex],
) {
    for column in reduced {
        let Some((pivot, _)) = column.pivot() else {
            continue;
        };
        for &row in column.0.keys() {
            if row != pivot {
                guards.insert(ReductionGuard {
                    kind: ReductionGuardKind::Pivot,
                    earlier: row_simplices[row].clone(),
                    later: row_simplices[pivot].clone(),
                });
            }
        }
    }
}

pub(in crate::certificate) fn minimize_guards(
    guards: BTreeSet<ReductionGuard>,
) -> Vec<ReductionGuard> {
    let mut pairs = BTreeMap::new();
    for guard in guards {
        pairs
            .entry((guard.earlier, guard.later))
            .and_modify(|kind: &mut ReductionGuardKind| *kind = (*kind).min(guard.kind))
            .or_insert(guard.kind);
    }
    let mut nodes = BTreeMap::new();
    for (earlier, later) in pairs.keys() {
        for simplex in [earlier, later] {
            let next = nodes.len();
            nodes.entry(simplex.clone()).or_insert(next);
        }
    }
    let edges: Vec<_> = pairs
        .into_iter()
        .map(|((earlier, later), kind)| {
            let source = nodes[&earlier];
            let target = nodes[&later];
            (
                source,
                target,
                ReductionGuard {
                    kind,
                    earlier,
                    later,
                },
            )
        })
        .collect();
    let mut outgoing = vec![Vec::new(); nodes.len()];
    for (source, target, _) in &edges {
        outgoing[*source].push(*target);
    }
    let mut reduced = BTreeSet::new();
    for (source, target, guard) in edges {
        let mut seen = vec![false; nodes.len()];
        let mut stack: Vec<_> = outgoing[source]
            .iter()
            .copied()
            .filter(|&next| next != target)
            .collect();
        let mut alternate = false;
        while let Some(node) = stack.pop() {
            if node == target {
                alternate = true;
                break;
            }
            if seen[node] {
                continue;
            }
            seen[node] = true;
            stack.extend(outgoing[node].iter().copied());
        }
        if !alternate {
            reduced.insert(guard);
        }
    }
    reduced.into_iter().collect()
}
