use std::collections::{BTreeMap, BTreeSet};

use crate::{ProofError, ProofLimits};

use super::linear::{Vector, rref};

pub(super) fn adjacency(vertex_count: usize, edges: &[super::Edge]) -> Vec<BTreeSet<usize>> {
    let mut adjacency = vec![BTreeSet::new(); vertex_count];
    for edge in edges {
        adjacency[edge.u].insert(edge.v);
        adjacency[edge.v].insert(edge.u);
    }
    adjacency
}

pub(super) fn flag_dimensions(
    vertex_count: usize,
    dimension: usize,
    adjacency: &[BTreeSet<usize>],
    limits: ProofLimits,
) -> Result<Vec<Vec<Vec<usize>>>, ProofError> {
    let vertices = (0..vertex_count)
        .map(|vertex| vec![vertex])
        .collect::<Vec<Vec<usize>>>();
    let mut dimensions = vec![vertices];
    for current in 1..=dimension + 1 {
        let next = flag_cofacets(
            current,
            &dimensions[current - 1],
            adjacency,
            simplex_limit(current, limits),
        )?;
        dimensions.push(next);
    }
    Ok(dimensions)
}

fn flag_cofacets(
    dimension: usize,
    simplices: &[Vec<usize>],
    adjacency: &[BTreeSet<usize>],
    maximum: usize,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let mut next = Vec::new();
    for simplex in simplices {
        let last = simplex.last().copied().unwrap_or(0);
        for &vertex in
            adjacency[last].range((std::ops::Bound::Excluded(last), std::ops::Bound::Unbounded))
        {
            if simplex[..simplex.len() - 1]
                .iter()
                .all(|member| adjacency[*member].contains(&vertex))
            {
                let mut cofacet = simplex.clone();
                cofacet.push(vertex);
                next.push(cofacet);
                if next.len() > maximum {
                    return Err(ProofError::new(format!(
                        "dimension {dimension} zigzag simplex count exceeds {maximum}"
                    )));
                }
            }
        }
    }
    Ok(next)
}

fn simplex_limit(dimension: usize, limits: ProofLimits) -> usize {
    match dimension {
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

pub(super) fn validate_incidence_count(
    dimensions: &[Vec<Vec<usize>>],
    dimension: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    let incidences = dimensions[dimension]
        .len()
        .checked_mul(dimension + 1)
        .and_then(|count| {
            dimensions[dimension + 1]
                .len()
                .checked_mul(dimension + 2)
                .and_then(|next| count.checked_add(next))
        })
        .ok_or_else(|| ProofError::new("zigzag incidence count overflows"))?;
    if incidences > limits.max_terms {
        Err(ProofError::new("zigzag incidence count exceeds its limit"))
    } else {
        Ok(())
    }
}

pub(super) fn coboundary_space(
    dimensions: &[Vec<Vec<usize>>],
    dimension: usize,
    q_simplices: &[Vec<usize>],
    modulus: u32,
) -> Result<Vec<Vector>, ProofError> {
    let rows = if dimension == 0 {
        Vec::new()
    } else {
        image_rows(&dimensions[dimension - 1], q_simplices, u64::from(modulus))?
    };
    Ok(rref(rows, u64::from(modulus)))
}

pub(super) fn quotient_basis(
    cocycles: Vec<Vector>,
    coboundaries: &[Vector],
    modulus: u32,
) -> Vec<Vector> {
    let mut quotient = Vec::new();
    for mut cocycle in cocycles {
        super::linear::reduce(&mut cocycle, coboundaries, u64::from(modulus));
        if !cocycle.is_zero() {
            quotient.push(cocycle);
        }
    }
    quotient
}

pub(super) fn boundary_row(
    simplex: &[usize],
    positions: &BTreeMap<Vec<usize>, usize>,
    modulus: u64,
) -> Result<Vector, ProofError> {
    let mut row = Vector::default();
    for removed in 0..simplex.len() {
        let mut face = simplex.to_vec();
        face.remove(removed);
        let position = positions
            .get(&face)
            .copied()
            .ok_or_else(|| ProofError::new("zigzag coboundary omits a face"))?;
        row.insert(position, sign(removed, modulus));
    }
    Ok(row)
}

pub(super) fn image_rows(
    faces: &[Vec<usize>],
    simplices: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<Vector>, ProofError> {
    let positions: BTreeMap<_, _> = faces
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let mut rows = vec![Vector::default(); faces.len()];
    for (simplex_position, simplex) in simplices.iter().enumerate() {
        for removed in 0..simplex.len() {
            let mut face = simplex.clone();
            face.remove(removed);
            let position = positions
                .get(&face)
                .copied()
                .ok_or_else(|| ProofError::new("zigzag boundary omits a face"))?;
            rows[position].insert(simplex_position, sign(removed, modulus));
        }
    }
    Ok(rows)
}

fn sign(position: usize, modulus: u64) -> u32 {
    if position % 2 == 0 {
        1
    } else {
        (modulus - 1) as u32
    }
}
