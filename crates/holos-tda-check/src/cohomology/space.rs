use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{ProofError, ProofLimits};

use super::complex::{
    adjacency, boundary_row, coboundary_space, flag_dimensions, quotient_basis,
    validate_incidence_count,
};
use super::linear::{Vector, nullspace, reduce, rref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Edge {
    pub(crate) u: usize,
    pub(crate) v: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MapTerm {
    pub(crate) target: usize,
    pub(crate) coefficient: u32,
}

pub(crate) struct Space {
    simplices: Vec<Vec<usize>>,
    coboundaries: Vec<Vector>,
    basis: Vec<Vector>,
}

impl Space {
    pub(crate) fn build(
        vertex_count: usize,
        dimension: usize,
        edges: &[Edge],
        modulus: u32,
        limits: ProofLimits,
    ) -> Result<Self, ProofError> {
        let adjacency = adjacency(vertex_count, edges);
        let dimensions = flag_dimensions(vertex_count, dimension, &adjacency, limits)?;
        validate_incidence_count(&dimensions, dimension, limits)?;
        let q_simplices = &dimensions[dimension];
        let positions: BTreeMap<_, _> = q_simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect();
        let equations = dimensions[dimension + 1]
            .iter()
            .map(|simplex| boundary_row(simplex, &positions, modulus as u64))
            .collect::<Result<Vec<_>, _>>()?;
        let cocycles = nullspace(equations, q_simplices.len(), modulus as u64);
        let coboundaries = coboundary_space(&dimensions, dimension, q_simplices, modulus)?;
        let quotient = quotient_basis(cocycles, &coboundaries, modulus);
        Ok(Self {
            simplices: q_simplices.clone(),
            coboundaries,
            basis: rref(quotient, modulus as u64),
        })
    }

    pub(crate) fn rank(&self) -> usize {
        self.basis.len()
    }

    pub(crate) fn coordinates_of_edge_cocycle(
        &self,
        terms: &[(Edge, u32)],
        modulus: u32,
    ) -> Result<Vec<MapTerm>, ProofError> {
        let positions = self
            .simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect::<BTreeMap<_, _>>();
        let mut vector = Vector::default();
        for &(edge, coefficient) in terms {
            let position = positions
                .get(&vec![edge.u, edge.v])
                .copied()
                .ok_or_else(|| ProofError::new("circular cocycle uses an inactive edge"))?;
            vector.insert(position, coefficient);
        }
        reduce(&mut vector, &self.coboundaries, u64::from(modulus));
        let coordinates = coordinates(&vector, &self.basis, u64::from(modulus))?;
        Ok(coordinates
            .0
            .into_iter()
            .map(|(target, coefficient)| MapTerm {
                target,
                coefficient,
            })
            .collect())
    }

    pub(crate) fn id(
        &self,
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        edges: &[Edge],
    ) -> [u8; 32] {
        let mut graph_hash = Sha256::new();
        graph_hash.update(b"holos-active-flag-graph-v1");
        graph_hash.update((vertex_count as u64).to_be_bytes());
        graph_hash.update(scale.to_bits().to_be_bytes());
        for edge in edges {
            graph_hash.update((edge.u as u64).to_be_bytes());
            graph_hash.update((edge.v as u64).to_be_bytes());
        }
        let graph_digest: [u8; 32] = graph_hash.finalize().into();
        let mut hash = Sha256::new();
        hash.update(b"holos-cohomology-space-v1");
        hash.update((vertex_count as u64).to_be_bytes());
        hash.update((dimension as u64).to_be_bytes());
        hash.update(scale.to_bits().to_be_bytes());
        hash.update(modulus.to_be_bytes());
        hash.update(graph_digest);
        hash.update((self.basis.len() as u64).to_be_bytes());
        for vector in &self.basis {
            hash.update((vector.0.len() as u64).to_be_bytes());
            for (&position, &coefficient) in &vector.0 {
                hash.update((self.simplices[position].len() as u64).to_be_bytes());
                for vertex in &self.simplices[position] {
                    hash.update((*vertex as u64).to_be_bytes());
                }
                hash.update(coefficient.to_be_bytes());
            }
        }
        hash.finalize().into()
    }

    pub(crate) fn canonical_subspace(
        &self,
        rows: &[Vec<MapTerm>],
        modulus: u32,
    ) -> Result<Vec<Vec<MapTerm>>, ProofError> {
        let vectors = rows
            .iter()
            .map(|row| {
                if row.windows(2).any(|pair| pair[0].target >= pair[1].target)
                    || row.iter().any(|term| {
                        term.target >= self.rank()
                            || term.coefficient == 0
                            || term.coefficient >= modulus
                    })
                {
                    return Err(ProofError::new(
                        "synthesis target coordinates are not canonical",
                    ));
                }
                let mut vector = Vector::default();
                for term in row {
                    vector.insert(term.target, term.coefficient);
                }
                Ok(vector)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rref(vectors, modulus as u64)
            .into_iter()
            .map(|row| {
                row.0
                    .into_iter()
                    .map(|(target, coefficient)| MapTerm {
                        target,
                        coefficient,
                    })
                    .collect()
            })
            .collect())
    }

    pub(crate) fn subspace_intersection_rank_from(
        &self,
        source: &Self,
        subspace: &[Vec<MapTerm>],
        modulus: u32,
    ) -> Result<usize, ProofError> {
        let (columns, _) = source.restriction_to(self, modulus)?;
        let image = columns
            .into_iter()
            .map(|column| {
                let mut row = Vector::default();
                for term in column {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let target = subspace
            .iter()
            .map(|terms| {
                let mut row = Vector::default();
                for term in terms {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let modulus = modulus as u64;
        let image_rank = rref(image.clone(), modulus).len();
        let target_rank = target.len();
        let union_rank = rref(image.into_iter().chain(target).collect(), modulus).len();
        image_rank
            .checked_add(target_rank)
            .and_then(|sum| sum.checked_sub(union_rank))
            .ok_or_else(|| ProofError::new("synthesis intersection rank is invalid"))
    }

    pub(crate) fn restriction_to(
        &self,
        target: &Self,
        modulus: u32,
    ) -> Result<(Vec<Vec<MapTerm>>, usize), ProofError> {
        let positions: BTreeMap<_, _> = target
            .simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect();
        let mut coordinate_rows = Vec::with_capacity(self.basis.len());
        let mut columns = Vec::with_capacity(self.basis.len());
        for vector in &self.basis {
            let mut restricted = Vector::default();
            for (&position, &coefficient) in &vector.0 {
                if let Some(&target_position) = positions.get(&self.simplices[position]) {
                    restricted.insert(target_position, coefficient);
                }
            }
            reduce(&mut restricted, &target.coboundaries, modulus as u64);
            let coordinates = coordinates(&restricted, &target.basis, modulus as u64)?;
            columns.push(
                coordinates
                    .0
                    .iter()
                    .map(|(&target, &coefficient)| MapTerm {
                        target,
                        coefficient,
                    })
                    .collect(),
            );
            coordinate_rows.push(coordinates);
        }
        let rank = rref(coordinate_rows, modulus as u64).len();
        Ok((columns, rank))
    }

    pub(crate) fn target_in_image_from(
        &self,
        source: &Self,
        target_basis: usize,
        modulus: u32,
    ) -> Result<bool, ProofError> {
        if target_basis >= self.basis.len() {
            return Err(ProofError::new(
                "cohomology intervention target basis is out of range",
            ));
        }
        let (columns, _) = source.restriction_to(self, modulus)?;
        let rows = columns
            .into_iter()
            .map(|column| {
                let mut row = Vector::default();
                for term in column {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect();
        let mut target = Vector::default();
        target.insert(target_basis, 1);
        reduce(&mut target, &rref(rows, modulus as u64), modulus as u64);
        Ok(target.is_zero())
    }
}

fn coordinates(vector: &Vector, basis: &[Vector], modulus: u64) -> Result<Vector, ProofError> {
    let mut residual = vector.clone();
    let mut output = Vector::default();
    for (position, basis_vector) in basis.iter().enumerate() {
        let pivot = basis_vector.leading().unwrap().0;
        if let Some(&coefficient) = residual.0.get(&pivot) {
            output.insert(position, coefficient);
            residual.add_scaled(basis_vector, modulus - u64::from(coefficient), modulus);
        }
    }
    if !residual.is_zero() {
        return Err(ProofError::new(
            "zigzag restriction is outside the target cohomology basis",
        ));
    }
    Ok(output)
}
