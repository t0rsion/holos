use crate::{Error, Result, SparseDistanceMatrix};

use super::algebra::{
    affine_solution, basis_positions, checked_coordinate_vector, combine_relation_new,
    continuation_result, coordinates_in_basis, full_relation, relation_terms, restricted_rows,
    rref,
};
use super::complex::{ActiveComplex, space_from_complex, validate};
use super::digest::{active_graph_digest, common_graph_digest};
use super::methods::checked_relation_row;
use super::model::*;

/// Continue one exact old class through a checked common-subcomplex relation.
pub fn cohomology_continuation(
    old: &CohomologySpace,
    new: &CohomologySpace,
    relation: &CohomologyRelation,
    old_coordinates: &[(usize, u32)],
) -> Result<CohomologyContinuation> {
    let relation_rows = validate_continuation_relation(old, new, relation)?;
    let selected = selected_class(old, old_coordinates)?;
    let equations = continuation_equations(&relation_rows, old.rank());
    let right = continuation_right(&selected, old.rank());
    let Some((particular, kernel)) =
        affine_solution(&equations, &right, relation_rows.len(), old.modulus as u64)
    else {
        return Ok(continuation_result(
            old,
            new,
            selected,
            CohomologyContinuationKind::NoExtension,
            SparseVector::default(),
            Vec::new(),
        ));
    };
    let target = combine_relation_new(&particular, &relation_rows, old.rank(), old.modulus as u64);
    let ambiguity = continuation_ambiguity(old, &kernel, &relation_rows);
    let kind = continuation_kind(&target, &ambiguity);
    Ok(continuation_result(
        old, new, selected, kind, target, ambiguity,
    ))
}

fn validate_continuation_relation(
    old: &CohomologySpace,
    new: &CohomologySpace,
    relation: &CohomologyRelation,
) -> Result<Vec<SparseVector>> {
    validate_continuation_space_compatibility(old, new)?;
    validate_continuation_header(old, new, relation)?;
    validate_continuation_ranks(relation)?;
    validate_continuation_basis(old, new, relation)
}

fn validate_continuation_space_compatibility(
    old: &CohomologySpace,
    new: &CohomologySpace,
) -> Result<()> {
    if old.vertex_count != new.vertex_count
        || old.dimension != new.dimension
        || old.scale.to_bits() != new.scale.to_bits()
        || old.modulus != new.modulus
    {
        return Err(Error::InvalidInput(
            "cohomology continuation spaces are incompatible".into(),
        ));
    }
    Ok(())
}

fn validate_continuation_header(
    old: &CohomologySpace,
    new: &CohomologySpace,
    relation: &CohomologyRelation,
) -> Result<()> {
    if relation.old_space != old.id
        || relation.new_space != new.id
        || relation.dimension != old.dimension
        || relation.scale.to_bits() != old.scale.to_bits()
        || relation.old_rank != old.rank()
        || relation.new_rank != new.rank()
        || relation.modulus != old.modulus
    {
        return Err(Error::InvalidInput(
            "cohomology continuation relation belongs to different spaces".into(),
        ));
    }
    Ok(())
}

fn validate_continuation_ranks(relation: &CohomologyRelation) -> Result<()> {
    if relation.relation_rank != relation.basis.len()
        || !rank_parts_match(
            relation.old_rank,
            relation.old_image_rank,
            relation.old_kernel_rank,
        )
        || !rank_parts_match(
            relation.new_rank,
            relation.new_image_rank,
            relation.new_kernel_rank,
        )
    {
        return Err(Error::InvalidInput(
            "cohomology continuation relation ranks are inconsistent".into(),
        ));
    }
    let minimum = relation
        .old_kernel_rank
        .checked_add(relation.new_kernel_rank)
        .ok_or_else(|| Error::InvalidInput("cohomology continuation rank overflows".into()))?;
    let maximum = relation
        .old_rank
        .checked_add(relation.new_rank)
        .ok_or_else(|| Error::InvalidInput("cohomology continuation rank overflows".into()))?;
    if relation.relation_rank < minimum || relation.relation_rank > maximum {
        return Err(Error::InvalidInput(
            "cohomology continuation relation rank is out of range".into(),
        ));
    }
    Ok(())
}

fn validate_continuation_basis(
    old: &CohomologySpace,
    new: &CohomologySpace,
    relation: &CohomologyRelation,
) -> Result<Vec<SparseVector>> {
    let rows = continuation_rows(old, new, relation)?;
    if rref(rows.clone(), old.modulus as u64).len() != relation.relation_rank {
        return Err(Error::InvalidInput(
            "cohomology continuation relation basis is not independent".into(),
        ));
    }
    Ok(rows)
}

fn rank_parts_match(rank: usize, image_rank: usize, kernel_rank: usize) -> bool {
    image_rank <= rank && kernel_rank <= rank && image_rank.checked_add(kernel_rank) == Some(rank)
}

fn selected_class(old: &CohomologySpace, old_coordinates: &[(usize, u32)]) -> Result<SparseVector> {
    let selected = checked_coordinate_vector(
        old_coordinates,
        old.rank(),
        old.modulus,
        "cohomology continuation coordinates are not canonical",
    )?;
    if selected.is_zero() {
        return Err(Error::InvalidInput(
            "cohomology continuation needs a nonzero old class".into(),
        ));
    }
    Ok(selected)
}

fn continuation_rows(
    old: &CohomologySpace,
    new: &CohomologySpace,
    relation: &CohomologyRelation,
) -> Result<Vec<SparseVector>> {
    let old_positions = basis_positions(old);
    let new_positions = basis_positions(new);
    relation
        .basis
        .iter()
        .map(|vector| {
            if vector.old.is_empty() && vector.new.is_empty() {
                return Err(Error::InvalidInput(
                    "cohomology relation contains a zero vector".into(),
                ));
            }
            let mut row = checked_relation_row(
                &vector.old,
                &old_positions,
                old.modulus,
                "cohomology relation old terms are not canonical",
            )?;
            let new_row = checked_relation_row(
                &vector.new,
                &new_positions,
                old.modulus,
                "cohomology relation new terms are not canonical",
            )?;
            for (position, coefficient) in new_row.0 {
                let position = old.rank().checked_add(position).ok_or_else(|| {
                    Error::InvalidInput("cohomology relation coordinate overflows".into())
                })?;
                row.insert(position, coefficient);
            }
            Ok(row)
        })
        .collect()
}

fn continuation_equations(relation_rows: &[SparseVector], old_rank: usize) -> Vec<SparseVector> {
    let mut equations = vec![SparseVector::default(); old_rank];
    for (variable, row) in relation_rows.iter().enumerate() {
        for (&position, &coefficient) in row.0.range(..old_rank) {
            equations[position].insert(variable, coefficient);
        }
    }
    equations
}

fn continuation_right(selected: &SparseVector, old_rank: usize) -> Vec<u32> {
    (0..old_rank)
        .map(|position| selected.0.get(&position).copied().unwrap_or(0))
        .collect()
}

fn continuation_ambiguity(
    old: &CohomologySpace,
    kernel: &[SparseVector],
    relation_rows: &[SparseVector],
) -> Vec<SparseVector> {
    rref(
        kernel
            .iter()
            .map(|coefficients| {
                combine_relation_new(coefficients, relation_rows, old.rank(), old.modulus as u64)
            })
            .collect(),
        old.modulus as u64,
    )
}

fn continuation_kind(
    target: &SparseVector,
    ambiguity: &[SparseVector],
) -> CohomologyContinuationKind {
    if !ambiguity.is_empty() {
        CohomologyContinuationKind::Ambiguous
    } else if target.is_zero() {
        CohomologyContinuationKind::NoNonzeroContinuation
    } else {
        CohomologyContinuationKind::Unique
    }
}

/// Compute a canonical cohomology space on the active flag complex.
pub fn cohomology_space(
    graph: &SparseDistanceMatrix,
    dimension: usize,
    scale: f64,
    modulus: u32,
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    validate(graph.len(), dimension, scale, modulus, limits)?;
    let active_edges = graph
        .edges()
        .filter(|&(_, _, value)| value <= scale)
        .map(|(u, v, _)| (u, v))
        .collect::<Vec<_>>();
    let complex = ActiveComplex::build(graph.len(), dimension, &active_edges, limits)?;
    space_from_complex(
        complex,
        graph.len(),
        dimension,
        scale,
        modulus,
        active_graph_digest(graph, scale),
        limits,
    )
}

/// Relate two canonical spaces by restriction to their common active flag subcomplex.
///
/// Restriction gives maps from both full cohomology spaces to the common
/// space. The returned basis is the complete kernel of the paired map
/// `[r_old, -r_new]`. It includes both restriction kernels.
pub fn cohomology_relation(
    old_graph: &SparseDistanceMatrix,
    old: &CohomologySpace,
    new_graph: &SparseDistanceMatrix,
    new: &CohomologySpace,
    limits: CohomologyLimits,
) -> Result<CohomologyRelation> {
    old.require_graph(old_graph)?;
    new.require_graph(new_graph)?;
    if old.vertex_count != new.vertex_count
        || old.dimension != new.dimension
        || old.modulus != new.modulus
        || old.scale.to_bits() != new.scale.to_bits()
    {
        return Err(Error::InvalidInput(
            "cohomology relation requires one vertex set, dimension, scale, and field".into(),
        ));
    }
    let common_edges = old_graph
        .edges()
        .filter(|&(u, v, value)| value <= old.scale && new_graph.get(u, v) <= old.scale)
        .map(|(u, v, _)| (u, v))
        .collect::<Vec<_>>();
    let common = ActiveComplex::build(old.vertex_count, old.dimension, &common_edges, limits)?;
    let common = space_from_complex(
        common,
        old.vertex_count,
        old.dimension,
        old.scale,
        old.modulus,
        common_graph_digest(old_graph, new_graph, old.scale),
        limits,
    )?;
    let old_rows = restricted_rows(old, &common);
    let new_rows = restricted_rows(new, &common);
    let modulus = old.modulus as u64;
    let old_image_rank = rref(old_rows.clone(), modulus).len();
    let new_image_rank = rref(new_rows.clone(), modulus).len();
    let relations = full_relation(&old_rows, &new_rows, common.simplices.len(), modulus);
    let basis = relations
        .into_iter()
        .map(
            |(old_coefficients, new_coefficients)| CohomologyRelationVector {
                old: relation_terms(old.basis(), &old_coefficients),
                new: relation_terms(new.basis(), &new_coefficients),
            },
        )
        .collect::<Vec<_>>();
    Ok(CohomologyRelation {
        old_space: old.id,
        new_space: new.id,
        dimension: old.dimension,
        scale: old.scale,
        modulus: old.modulus,
        old_rank: old.rank(),
        new_rank: new.rank(),
        old_image_rank,
        new_image_rank,
        old_kernel_rank: old.rank() - old_image_rank,
        new_kernel_rank: new.rank() - new_image_rank,
        relation_rank: basis.len(),
        basis,
    })
}

/// Compute the cohomology restriction from a complex to an active subcomplex.
///
/// Every active edge of `target_graph` must also be active in `source_graph`.
/// The returned columns use the canonical bases of `source` and `target`.
pub fn cohomology_restriction(
    source_graph: &SparseDistanceMatrix,
    source: &CohomologySpace,
    target_graph: &SparseDistanceMatrix,
    target: &CohomologySpace,
) -> Result<CohomologyRestriction> {
    source.require_graph(source_graph)?;
    target.require_graph(target_graph)?;
    if source.vertex_count != target.vertex_count
        || source.dimension != target.dimension
        || source.modulus != target.modulus
        || source.scale.to_bits() != target.scale.to_bits()
    {
        return Err(Error::InvalidInput(
            "cohomology restriction requires one vertex set, dimension, scale, and field".into(),
        ));
    }
    if target_graph
        .edges()
        .any(|(u, v, value)| value <= target.scale && source_graph.get(u, v) > source.scale)
    {
        return Err(Error::InvalidInput(
            "cohomology restriction target is not an active subcomplex".into(),
        ));
    }
    let modulus = source.modulus as u64;
    let images = restricted_rows(source, target);
    let coordinates = images
        .iter()
        .map(|image| coordinates_in_basis(image, &target.basis_vectors, modulus))
        .collect::<Result<Vec<_>>>()?;
    let rank = rref(coordinates.clone(), modulus).len();
    let columns = source
        .basis
        .iter()
        .zip(coordinates)
        .map(|(class, coordinates)| CohomologyMapColumn {
            source: class.id,
            image: coordinates
                .0
                .into_iter()
                .map(|(position, coefficient)| CohomologyMapTerm {
                    class: target.basis[position].id,
                    coefficient,
                })
                .collect(),
        })
        .collect();
    Ok(CohomologyRestriction {
        source_space: source.id,
        target_space: target.id,
        dimension: source.dimension,
        scale: source.scale,
        modulus: source.modulus,
        rank,
        columns,
    })
}
