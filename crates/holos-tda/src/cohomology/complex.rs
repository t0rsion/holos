use std::collections::BTreeMap;

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result};

use super::algebra::{nullspace, reduce_by_basis, rref};
use super::digest::{class_id, space_id};
use super::forest::forest_cocycles;
use super::model::*;

pub(crate) fn validate(
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    limits: CohomologyLimits,
) -> Result<()> {
    if vertex_count > limits.max_vertices {
        return Err(Error::InvalidInput(format!(
            "cohomology vertex count exceeds the limit {}",
            limits.max_vertices
        )));
    }
    if dimension > limits.max_dimension {
        return Err(Error::InvalidInput(format!(
            "cohomology dimension exceeds the limit {}",
            limits.max_dimension
        )));
    }
    checked_dimension_slots(dimension)?;
    if !scale.is_finite() || scale < 0.0 {
        return Err(Error::InvalidInput(
            "cohomology scale must be finite and non-negative".into(),
        ));
    }
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(
            "cohomology modulus must be a supported prime".into(),
        ));
    }
    Ok(())
}

pub(crate) struct ActiveComplex {
    dimensions: Vec<Vec<Vec<usize>>>,
}

impl ActiveComplex {
    pub(crate) fn build(
        vertex_count: usize,
        max_dimension: usize,
        edges: &[(usize, usize)],
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let dimension_slots = checked_dimension_slots(max_dimension)?;
        let dimension_count = dimension_slots - 1;
        // Keep one slot for every dimension through the requested cofacet
        // dimension before allocating the graph adjacency.
        let mut dimensions = Vec::new();
        dimensions.try_reserve_exact(dimension_slots).map_err(|_| {
            Error::InvalidInput("cohomology dimension storage cannot be allocated".into())
        })?;
        let mut adjacency = vec![Vec::new(); vertex_count];
        for &(u, v) in edges {
            adjacency[u].push(v);
            adjacency[v].push(u);
        }
        for neighbors in &mut adjacency {
            neighbors.sort_unstable();
        }
        let vertices: Vec<_> = (0..vertex_count).map(|vertex| vec![vertex]).collect();
        if vertices.len() > limits.max_simplices_per_dimension {
            return Err(Error::InvalidInput(
                "cohomology vertex simplex count exceeds its limit".into(),
            ));
        }
        dimensions.push(vertices);
        extend_dimensions(
            &mut dimensions,
            dimension_count,
            &adjacency,
            limits.max_simplices_per_dimension,
        )?;
        Ok(Self { dimensions })
    }
}

fn checked_dimension_slots(dimension: usize) -> Result<usize> {
    dimension
        .checked_add(2)
        .ok_or_else(|| Error::InvalidInput("cohomology dimension metadata overflows".into()))
}

fn extend_dimensions(
    dimensions: &mut Vec<Vec<Vec<usize>>>,
    dimension_count: usize,
    adjacency: &[Vec<usize>],
    simplex_limit: usize,
) -> Result<()> {
    for dimension in 1..=dimension_count {
        let next = extend_dimension(
            &dimensions[dimension - 1],
            adjacency,
            dimension,
            simplex_limit,
        )?;
        dimensions.push(next);
    }
    Ok(())
}

fn extend_dimension(
    simplices: &[Vec<usize>],
    adjacency: &[Vec<usize>],
    dimension: usize,
    simplex_limit: usize,
) -> Result<Vec<Vec<usize>>> {
    let mut next = Vec::new();
    for simplex in simplices {
        let last = simplex.last().copied().unwrap_or(0);
        let start = adjacency[last].partition_point(|&vertex| vertex <= last);
        for &vertex in &adjacency[last][start..] {
            if simplex[..simplex.len() - 1]
                .iter()
                .all(|&member| adjacency[member].binary_search(&vertex).is_ok())
            {
                let mut cofacet = simplex.clone();
                cofacet.push(vertex);
                next.push(cofacet);
                if next.len() > simplex_limit {
                    return Err(Error::InvalidInput(format!(
                        "dimension {dimension} cohomology simplex count exceeds the limit {simplex_limit}"
                    )));
                }
            }
        }
    }
    Ok(next)
}

pub(crate) fn space_from_complex(
    complex: ActiveComplex,
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    active_graph_digest: [u8; 32],
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    space_from_complex_with_method::<true>(
        complex,
        vertex_count,
        dimension,
        scale,
        modulus,
        active_graph_digest,
        limits,
    )
}

#[cfg(test)]
pub(crate) fn space_from_complex_nullspace(
    complex: ActiveComplex,
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    active_graph_digest: [u8; 32],
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    space_from_complex_with_method::<false>(
        complex,
        vertex_count,
        dimension,
        scale,
        modulus,
        active_graph_digest,
        limits,
    )
}

fn space_from_complex_with_method<const FOREST_GAUGE: bool>(
    complex: ActiveComplex,
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    active_graph_digest: [u8; 32],
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    check_incidence_budget(&complex, dimension, limits.max_boundary_terms)?;
    let modulus64 = modulus as u64;
    let q_simplices = &complex.dimensions[dimension];
    let cocycles =
        cocycle_vectors::<FOREST_GAUGE>(&complex, vertex_count, dimension, q_simplices, modulus64)?;
    let coboundaries = if dimension == 0 {
        Vec::new()
    } else {
        coboundary_image(&complex.dimensions[dimension - 1], q_simplices, modulus64)?
    };
    let coboundaries = rref(coboundaries, modulus64);
    let basis_vectors = quotient_basis(cocycles, &coboundaries, modulus64);
    let simplex_counts = complex.dimensions.iter().map(Vec::len).collect::<Vec<_>>();
    let id = space_id(
        vertex_count,
        dimension,
        scale,
        modulus,
        &active_graph_digest,
        q_simplices,
        &basis_vectors,
    );
    let basis = canonical_classes(id, q_simplices, &basis_vectors);
    Ok(CohomologySpace {
        id,
        vertex_count,
        dimension,
        scale,
        modulus,
        active_graph_digest,
        simplex_counts,
        simplices: q_simplices.clone(),
        coboundaries,
        basis_vectors,
        basis,
    })
}

fn check_incidence_budget(
    complex: &ActiveComplex,
    dimension: usize,
    max_boundary_terms: usize,
) -> Result<()> {
    let incidence_count = complex.dimensions[dimension]
        .len()
        .checked_mul(dimension + 1)
        .and_then(|count| {
            complex.dimensions[dimension + 1]
                .len()
                .checked_mul(dimension + 2)
                .and_then(|next| count.checked_add(next))
        })
        .ok_or_else(|| Error::InvalidInput("cohomology incidence count overflows".into()))?;
    if incidence_count > max_boundary_terms {
        return Err(Error::InvalidInput(format!(
            "cohomology incidence count exceeds the limit {max_boundary_terms}"
        )));
    }
    Ok(())
}

fn cocycle_vectors<const FOREST_GAUGE: bool>(
    complex: &ActiveComplex,
    vertex_count: usize,
    dimension: usize,
    q_simplices: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    if FOREST_GAUGE && dimension == 1 {
        return forest_cocycles(
            vertex_count,
            q_simplices,
            &complex.dimensions[dimension + 1],
            modulus,
        );
    }
    let q_positions: BTreeMap<_, _> = q_simplices
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let cocycle_equations =
        coboundary_equations(&complex.dimensions[dimension + 1], &q_positions, modulus)?;
    Ok(nullspace(cocycle_equations, q_simplices.len(), modulus))
}

fn quotient_basis(
    cocycles: Vec<SparseVector>,
    coboundaries: &[SparseVector],
    modulus: u64,
) -> Vec<SparseVector> {
    let mut quotient = Vec::new();
    for mut cocycle in cocycles {
        reduce_by_basis(&mut cocycle, coboundaries, modulus);
        if !cocycle.is_zero() {
            quotient.push(cocycle);
        }
    }
    rref(quotient, modulus)
}

fn canonical_classes(
    id: CohomologySpaceId,
    q_simplices: &[Vec<usize>],
    basis_vectors: &[SparseVector],
) -> Vec<CohomologyClass> {
    basis_vectors
        .iter()
        .enumerate()
        .map(|(basis_index, vector)| CohomologyClass {
            id: class_id(id, basis_index, vector),
            basis_index,
            terms: vector
                .0
                .iter()
                .map(|(&position, &coefficient)| CochainTerm {
                    simplex: q_simplices[position].clone(),
                    coefficient,
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn coboundary_equations(
    cofacets: &[Vec<usize>],
    face_positions: &BTreeMap<Vec<usize>, usize>,
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    cofacets
        .iter()
        .map(|cofacet| {
            let mut equation = SparseVector::default();
            for removed in 0..cofacet.len() {
                let mut face = cofacet.clone();
                face.remove(removed);
                let position = face_positions.get(&face).copied().ok_or_else(|| {
                    Error::InvalidInput("cohomology coboundary omits a face".into())
                })?;
                equation.insert(position, sign(removed, modulus));
            }
            Ok(equation)
        })
        .collect()
}

pub(crate) fn coboundary_image(
    faces: &[Vec<usize>],
    simplices: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    let face_positions: BTreeMap<_, _> = faces
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let mut generators = vec![SparseVector::default(); faces.len()];
    for (simplex_position, simplex) in simplices.iter().enumerate() {
        for removed in 0..simplex.len() {
            let mut face = simplex.clone();
            face.remove(removed);
            let position = face_positions
                .get(&face)
                .copied()
                .ok_or_else(|| Error::InvalidInput("cohomology boundary omits a face".into()))?;
            generators[position].insert(simplex_position, sign(removed, modulus));
        }
    }
    Ok(generators)
}

pub(crate) fn sign(position: usize, modulus: u64) -> u32 {
    if position % 2 == 0 {
        1
    } else {
        (modulus - 1) as u32
    }
}
