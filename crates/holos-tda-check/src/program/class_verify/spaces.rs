use crate::proof::{Graph, ProofBar, ProofError};

use super::super::claim::{AtlasClaim, CriticalPairClaim, SpaceClaim};
use super::super::model::ProgramProofLimits;
use super::super::reduction::H1Pair;
use super::basis::canonical_basis;
use super::identity::{
    basis_class_id, declared_pair_order, group_id, pair_order, previous_float, terminal_level,
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
    check_declared_pairs(checked_pairs, &mut declared_pairs)
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
    let cocycles = check_basis_classes(space_index, space, graph, modulus, limits, expected_scale)?;
    let canonical = canonical_basis(graph, modulus, expected_scale, &cocycles)?;
    if canonical != cocycles {
        return Err(ProofError::new(format!(
            "atlas space {space_index} basis is not canonical"
        )));
    }
    let group_id = group_id(space.interval, modulus, expected_scale, &cocycles);
    if space.id != group_id {
        return Err(ProofError::new(format!(
            "atlas space {space_index} identifier does not match its basis"
        )));
    }
    check_basis_ids(space_index, space, group_id)
}

fn check_basis_classes(
    space_index: usize,
    space: &SpaceClaim,
    graph: &Graph,
    modulus: u32,
    limits: ProgramProofLimits,
    expected_scale: f64,
) -> Result<Vec<Vec<super::super::claim::CocycleTermClaim>>, ProofError> {
    let mut cocycles = Vec::with_capacity(space.basis.len());
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
        cocycles.push(class.terms.clone());
    }
    Ok(cocycles)
}

fn check_basis_ids(
    space_index: usize,
    space: &SpaceClaim,
    group_id: [u8; 32],
) -> Result<(), ProofError> {
    for (basis_index, class) in space.basis.iter().enumerate() {
        if class.id != basis_class_id(group_id, basis_index, class.scale, &class.terms) {
            return Err(ProofError::new(format!(
                "atlas space {space_index} basis identifier is not canonical"
            )));
        }
    }
    Ok(())
}
