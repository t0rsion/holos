use std::collections::BTreeMap;

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result};

use super::algebra::{nullspace, reduce_by_basis, rref};
use super::digest::{class_id, space_id};
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
        let mut dimensions = vec![vertices];
        for dimension in 1..=max_dimension + 1 {
            let mut next = Vec::new();
            for simplex in &dimensions[dimension - 1] {
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
                        if next.len() > limits.max_simplices_per_dimension {
                            return Err(Error::InvalidInput(format!(
                                "dimension {dimension} cohomology simplex count exceeds the limit {}",
                                limits.max_simplices_per_dimension
                            )));
                        }
                    }
                }
            }
            dimensions.push(next);
        }
        Ok(Self { dimensions })
    }
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
    if incidence_count > limits.max_boundary_terms {
        return Err(Error::InvalidInput(format!(
            "cohomology incidence count exceeds the limit {}",
            limits.max_boundary_terms
        )));
    }
    let modulus64 = modulus as u64;
    let q_simplices = &complex.dimensions[dimension];
    let q_positions: BTreeMap<_, _> = q_simplices
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let cocycle_equations =
        coboundary_equations(&complex.dimensions[dimension + 1], &q_positions, modulus64)?;
    let cocycles = nullspace(cocycle_equations, q_simplices.len(), modulus64);
    let coboundaries = if dimension == 0 {
        Vec::new()
    } else {
        coboundary_image(&complex.dimensions[dimension - 1], q_simplices, modulus64)?
    };
    let coboundaries = rref(coboundaries, modulus64);
    let mut quotient = Vec::new();
    for mut cocycle in cocycles {
        reduce_by_basis(&mut cocycle, &coboundaries, modulus64);
        if !cocycle.is_zero() {
            quotient.push(cocycle);
        }
    }
    let basis_vectors = rref(quotient, modulus64);
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
    let basis = basis_vectors
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
        .collect();
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
