use crate::proof::{Graph, ProofBar, ProofError};

use super::super::claim::{AtlasClaim, CocycleTermClaim, CriticalPairClaim, SpaceClaim};
use super::super::model::ProgramProofLimits;
use super::super::reduction::H1Pair;
use super::super::{ReplayCocycleTerm, ReplayGroup, ReplayPair, ReplayResult, replay_h1};
use super::identity::{
    bar_bits_equal, declared_pair_order, pair_order, previous_float, terminal_level,
};
use super::validation::{check_bar, check_cocycle_shape, check_provenance, check_simplex};

pub(crate) fn check_spaces(
    atlas: &AtlasClaim,
    graph: &Graph,
    checked_pairs: &[H1Pair],
    limits: ProgramProofLimits,
) -> Result<(), ProofError> {
    let mut previous: Option<(f64, f64, [u8; 32])> = None;
    let mut interval_records = Vec::new();
    let mut declared_pairs = Vec::new();
    for (space_index, space) in atlas.spaces.iter().enumerate() {
        let current = (space.interval.birth, space.interval.death, space.id);
        if previous.is_some_and(|previous| {
            current
                .0
                .total_cmp(&previous.0)
                .then(current.1.total_cmp(&previous.1))
                .then(current.2.cmp(&previous.2))
                != std::cmp::Ordering::Greater
        }) {
            return Err(ProofError::new(
                "atlas class spaces are not in canonical order",
            ));
        }
        previous = Some(current);
        let multiplicity = check_space(
            space_index,
            space,
            graph,
            atlas.threshold,
            atlas.modulus,
            limits,
            &mut declared_pairs,
        )?;
        interval_records.extend(std::iter::repeat_n(
            (
                space.interval.birth.to_bits(),
                space.interval.death.to_bits(),
            ),
            multiplicity,
        ));
    }
    interval_records.sort_unstable();
    check_space_intervals(atlas, &interval_records)?;
    check_declared_pairs(checked_pairs, &mut declared_pairs)?;
    let replay = replay_h1(graph, atlas.threshold, atlas.modulus, limits)?;
    check_replay_linkage(atlas, &replay)
}

fn check_replay_linkage(atlas: &AtlasClaim, replay: &ReplayResult) -> Result<(), ProofError> {
    if replay.groups.len() != atlas.spaces.len() {
        return Err(ProofError::new(
            "atlas class-space count differs from independent replay",
        ));
    }
    let mut used = vec![false; replay.groups.len()];
    for (space_index, space) in atlas.spaces.iter().enumerate() {
        let Some((group_index, group)) =
            replay
                .groups
                .iter()
                .enumerate()
                .find(|(group_index, group)| {
                    !used[*group_index] && bar_bits_equal(space.interval, group.interval)
                })
        else {
            return Err(ProofError::new(format!(
                "atlas class space {space_index} has no independent replay group"
            )));
        };
        used[group_index] = true;
        check_replay_group(space_index, space, group)?;
    }
    Ok(())
}

fn check_replay_group(
    space_index: usize,
    space: &SpaceClaim,
    group: &ReplayGroup,
) -> Result<(), ProofError> {
    check_replay_identity(space_index, space, group)?;
    check_replay_multiplicity(space_index, space, group)?;
    check_replay_pairs(space_index, space, group)?;
    check_replay_basis(space_index, space, group)?;
    Ok(())
}

fn check_replay_identity(
    space_index: usize,
    space: &SpaceClaim,
    group: &ReplayGroup,
) -> Result<(), ProofError> {
    if space.id != group.id {
        return Err(ProofError::new(format!(
            "atlas space {space_index} identifier differs from independent replay"
        )));
    }
    Ok(())
}

fn check_replay_multiplicity(
    space_index: usize,
    space: &SpaceClaim,
    group: &ReplayGroup,
) -> Result<(), ProofError> {
    if space.critical_pairs.len() != group.pairs.len()
        || space.basis.len() != group.basis.len()
        || group.basis.len() != group.class_ids.len()
    {
        return Err(ProofError::new(format!(
            "atlas space {space_index} multiplicity differs from independent replay"
        )));
    }
    Ok(())
}

fn check_replay_pairs(
    space_index: usize,
    space: &SpaceClaim,
    group: &ReplayGroup,
) -> Result<(), ProofError> {
    for (pair_index, (declared, expected)) in
        space.critical_pairs.iter().zip(&group.pairs).enumerate()
    {
        if !replay_pair_matches(declared, expected, group.interval) {
            return Err(ProofError::new(format!(
                "atlas space {space_index} critical pair {pair_index} differs from independent replay"
            )));
        }
    }
    Ok(())
}

fn check_replay_basis(
    space_index: usize,
    space: &SpaceClaim,
    group: &ReplayGroup,
) -> Result<(), ProofError> {
    for (basis_index, (class, expected_terms)) in space.basis.iter().zip(&group.basis).enumerate() {
        if class.scale.to_bits() != group.scale.to_bits()
            || class.basis_index != basis_index
            || class.id != group.class_ids[basis_index]
            || !replay_terms_match(&class.terms, expected_terms)
        {
            return Err(ProofError::new(format!(
                "atlas space {space_index} basis class {basis_index} differs from independent replay"
            )));
        }
    }
    Ok(())
}

fn replay_pair_matches(
    declared: &CriticalPairClaim,
    expected: &ReplayPair,
    interval: ProofBar,
) -> bool {
    if !bar_bits_equal(expected.interval, interval)
        || declared.birth.vertices.as_slice() != expected.birth
        || declared.birth.value.to_bits() != expected.interval.birth.to_bits()
    {
        return false;
    }
    match (&declared.death, &expected.death) {
        (None, None) => true,
        (Some(declared), Some(expected)) => {
            declared.vertices.as_slice() == expected
                && declared.value.to_bits() == interval.death.to_bits()
        }
        _ => false,
    }
}

fn replay_terms_match(declared: &[CocycleTermClaim], expected: &[ReplayCocycleTerm]) -> bool {
    declared.len() == expected.len()
        && declared.iter().zip(expected).all(|(declared, expected)| {
            declared.u == expected.u
                && declared.v == expected.v
                && declared.coefficient == expected.coefficient
        })
}

fn check_space(
    space_index: usize,
    space: &SpaceClaim,
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    limits: ProgramProofLimits,
    declared: &mut Vec<(ProofBar, CriticalPairClaim)>,
) -> Result<usize, ProofError> {
    check_bar(&space.interval)?;
    if space.basis.is_empty() || space.basis.len() != space.critical_pairs.len() {
        return Err(ProofError::new(format!(
            "atlas space {space_index} has inconsistent multiplicity"
        )));
    }
    check_space_pairs(space_index, space, graph, declared)?;
    check_space_basis(space_index, space, graph, threshold, modulus, limits)?;
    Ok(space.basis.len())
}

fn check_space_intervals(
    atlas: &AtlasClaim,
    interval_records: &[(u64, u64)],
) -> Result<(), ProofError> {
    let mut atlas_h1: Vec<_> = atlas
        .diagram
        .iter()
        .filter(|bar| bar.dimension == 1)
        .map(|bar| (bar.birth.to_bits(), bar.death.to_bits()))
        .collect();
    atlas_h1.sort_unstable();
    if atlas_h1 != interval_records {
        return Err(ProofError::new(
            "atlas class-space intervals do not match its H1 diagram",
        ));
    }
    Ok(())
}

fn check_declared_pairs(
    checked_pairs: &[H1Pair],
    declared_pairs: &mut [(ProofBar, CriticalPairClaim)],
) -> Result<(), ProofError> {
    let mut checked = checked_pairs.to_vec();
    checked.sort_by(pair_order);
    declared_pairs.sort_by(declared_pair_order);
    if checked.len() != declared_pairs.len()
        || checked.iter().zip(declared_pairs).any(|(left, right)| {
            left.interval.birth.to_bits() != right.0.birth.to_bits()
                || left.interval.death.to_bits() != right.0.death.to_bits()
                || left.birth != right.1.birth.vertices.as_slice()
                || left.death.as_ref().map(|death| death.as_slice())
                    != right
                        .1
                        .death
                        .as_ref()
                        .map(|simplex| simplex.vertices.as_slice())
        })
    {
        return Err(ProofError::new(
            "atlas critical pairs differ from the checked reduction",
        ));
    }
    Ok(())
}

fn check_space_pairs(
    space_index: usize,
    space: &SpaceClaim,
    graph: &Graph,
    declared: &mut Vec<(ProofBar, CriticalPairClaim)>,
) -> Result<(), ProofError> {
    let mut previous = None;
    for pair in &space.critical_pairs {
        check_simplex(&pair.birth, 2, graph)?;
        if pair.birth.value.to_bits() != space.interval.birth.to_bits() {
            return Err(ProofError::new(format!(
                "atlas space {space_index} birth value differs from its interval"
            )));
        }
        match (&pair.death, space.interval.death.is_infinite()) {
            (None, true) => {}
            (Some(death), false) => {
                check_simplex(death, 3, graph)?;
                if death.value.to_bits() != space.interval.death.to_bits() {
                    return Err(ProofError::new(format!(
                        "atlas space {space_index} death value differs from its interval"
                    )));
                }
            }
            _ => {
                return Err(ProofError::new(format!(
                    "atlas space {space_index} has the wrong death presence"
                )));
            }
        }
        if previous.is_some_and(|previous: (&[usize], Option<&[usize]>)| {
            super::identity::critical_pair_order(previous.0, previous.1, pair)
                != std::cmp::Ordering::Less
        }) {
            return Err(ProofError::new(format!(
                "atlas space {space_index} critical pairs are not canonical"
            )));
        }
        previous = Some((
            pair.birth.vertices.as_slice(),
            pair.death.as_ref().map(|death| death.vertices.as_slice()),
        ));
        declared.push((space.interval, pair.clone()));
    }
    Ok(())
}

fn check_space_basis(
    space_index: usize,
    space: &SpaceClaim,
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    limits: ProgramProofLimits,
) -> Result<(), ProofError> {
    let expected_scale = if space.interval.death.is_finite() {
        previous_float(space.interval.death)
    } else {
        terminal_level(graph, threshold)
    };
    check_basis_classes(space_index, space, graph, modulus, limits, expected_scale)
}

fn check_basis_classes(
    space_index: usize,
    space: &SpaceClaim,
    graph: &Graph,
    modulus: u32,
    limits: ProgramProofLimits,
    expected_scale: f64,
) -> Result<(), ProofError> {
    for (basis_index, class) in space.basis.iter().enumerate() {
        if class.basis_index != basis_index || class.scale.to_bits() != expected_scale.to_bits() {
            return Err(ProofError::new(format!(
                "atlas space {space_index} basis index or scale is not canonical"
            )));
        }
        check_cocycle_shape(class, graph, modulus, limits)?;
        if let Some(provenance) = class.provenance.as_ref() {
            check_provenance(provenance, class, graph, space.interval, modulus)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::identity::basis_class_id;
    use super::check_replay_linkage;
    use crate::program::claim::{
        AtlasClaim, ClassClaim, CocycleTermClaim, CriticalPairClaim, ReductionClaim, SimplexClaim,
        SpaceClaim,
    };
    use crate::program::{ProgramProofLimits, ReplayCocycleTerm, ReplayGroup, replay_h1};
    use crate::proof::{Graph, ProofEdge};

    fn graph(vertex_count: usize, edges: &[(usize, usize)]) -> Graph {
        let edges = edges
            .iter()
            .map(|&(u, v)| ProofEdge { u, v, value: 1.0 })
            .collect::<Vec<_>>();
        Graph::new(vertex_count, &edges).unwrap()
    }

    fn atlas_for_group(group: &ReplayGroup, modulus: u32, vertex_count: usize) -> AtlasClaim {
        let critical_pairs = group
            .pairs
            .iter()
            .map(|pair| CriticalPairClaim {
                birth: SimplexClaim {
                    vertices: pair.birth.to_vec(),
                    value: pair.interval.birth,
                },
                death: pair.death.map(|vertices| SimplexClaim {
                    vertices: vertices.to_vec(),
                    value: pair.interval.death,
                }),
            })
            .collect();
        let basis = group
            .basis
            .iter()
            .enumerate()
            .map(|(basis_index, terms)| ClassClaim {
                id: group.class_ids[basis_index],
                basis_index,
                scale: group.scale,
                terms: terms
                    .iter()
                    .map(|term| CocycleTermClaim {
                        u: term.u,
                        v: term.v,
                        coefficient: term.coefficient,
                    })
                    .collect(),
                provenance: None,
            })
            .collect();
        AtlasClaim {
            modulus,
            vertex_count,
            threshold: None,
            input_digest: [0; 32],
            diagram: vec![group.interval],
            spaces: vec![SpaceClaim {
                id: group.id,
                interval: group.interval,
                critical_pairs,
                basis,
            }],
            reduction: ReductionClaim {
                modulus,
                vertex_count,
                threshold: None,
                graph_digest: [0; 32],
                edge_columns: Vec::new(),
                triangle_columns: Vec::new(),
                diagram: Vec::new(),
            },
        }
    }

    fn mixed_terms(
        left: &[ReplayCocycleTerm],
        right: &[ReplayCocycleTerm],
        modulus: u32,
    ) -> Vec<CocycleTermClaim> {
        let mut coefficients = BTreeMap::new();
        for term in left.iter().chain(right) {
            let entry = coefficients.entry((term.u, term.v)).or_insert(0u64);
            *entry = (*entry + u64::from(term.coefficient)) % u64::from(modulus);
        }
        let mut terms: Vec<_> = coefficients
            .into_iter()
            .filter_map(|((u, v), coefficient)| {
                (coefficient != 0).then_some(CocycleTermClaim {
                    u,
                    v,
                    coefficient: coefficient as u32,
                })
            })
            .collect();
        let inverse = crate::inverse_mod(u64::from(terms[0].coefficient), u64::from(modulus));
        for term in &mut terms {
            term.coefficient = (u64::from(term.coefficient) * inverse % u64::from(modulus)) as u32;
        }
        terms
    }

    #[test]
    fn replay_linkage_rejects_a_mixed_cocycle() {
        let graph = graph(4, &[(0, 1), (0, 3), (1, 2), (2, 3)]);
        let replay = replay_h1(&graph, None, 2, ProgramProofLimits::default()).unwrap();
        let atlas = atlas_for_group(&replay.groups[0], 2, 4);
        check_replay_linkage(&atlas, &replay).unwrap();

        let mut mixed = atlas;
        mixed.spaces[0].basis[0].terms[0].u = 0;
        assert!(check_replay_linkage(&mixed, &replay).is_err());
    }

    #[test]
    fn replay_linkage_rejects_a_rehashed_sum_of_two_classes() {
        let graph = graph(
            7,
            &[
                (0, 1),
                (1, 2),
                (2, 3),
                (0, 3),
                (0, 4),
                (4, 5),
                (5, 6),
                (0, 6),
            ],
        );
        let replay = replay_h1(&graph, None, 3, ProgramProofLimits::default()).unwrap();
        assert_eq!(replay.groups[0].basis.len(), 2);
        let atlas = atlas_for_group(&replay.groups[0], 3, 7);
        check_replay_linkage(&atlas, &replay).unwrap();

        let mut mixed = atlas;
        let terms = mixed_terms(&replay.groups[0].basis[0], &replay.groups[0].basis[1], 3);
        mixed.spaces[0].basis[0].terms = terms;
        mixed.spaces[0].basis[0].id = basis_class_id(
            mixed.spaces[0].id,
            0,
            mixed.spaces[0].basis[0].scale,
            &mixed.spaces[0].basis[0].terms,
        );
        assert!(check_replay_linkage(&mixed, &replay).is_err());
    }
}
