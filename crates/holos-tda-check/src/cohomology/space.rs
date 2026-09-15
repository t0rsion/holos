use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
        build_space(
            vertex_count,
            dimension,
            edges,
            modulus,
            limits,
            dimension == 1,
        )
    }

    #[cfg(test)]
    fn build_unrestricted(
        vertex_count: usize,
        dimension: usize,
        edges: &[Edge],
        modulus: u32,
        limits: ProofLimits,
    ) -> Result<Self, ProofError> {
        build_space(vertex_count, dimension, edges, modulus, limits, false)
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

fn build_space(
    vertex_count: usize,
    dimension: usize,
    edges: &[Edge],
    modulus: u32,
    limits: ProofLimits,
    gauge_h1: bool,
) -> Result<Space, ProofError> {
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
    let cocycles = if gauge_h1 {
        gauged_h1_cocycles(
            vertex_count,
            q_simplices,
            &dimensions[dimension + 1],
            &positions,
            &adjacency,
            modulus as u64,
        )?
    } else {
        let equations = dimensions[dimension + 1]
            .iter()
            .map(|simplex| boundary_row(simplex, &positions, modulus as u64))
            .collect::<Result<Vec<_>, _>>()?;
        nullspace(equations, q_simplices.len(), modulus as u64)
    };
    let coboundaries = coboundary_space(&dimensions, dimension, q_simplices, modulus)?;
    let quotient = quotient_basis(cocycles, &coboundaries, modulus);
    Ok(Space {
        simplices: q_simplices.clone(),
        coboundaries,
        basis: rref(quotient, modulus as u64),
    })
}

fn gauged_h1_cocycles(
    vertex_count: usize,
    edges: &[Vec<usize>],
    triangles: &[Vec<usize>],
    positions: &BTreeMap<Vec<usize>, usize>,
    adjacency: &[BTreeSet<usize>],
    modulus: u64,
) -> Result<Vec<Vector>, ProofError> {
    let forest = spanning_forest(vertex_count, adjacency);
    // Every cocycle class has one representative with zero forest-edge values.
    let mut cotree_positions = vec![None; edges.len()];
    let mut cotree = Vec::new();
    for (position, simplex) in edges.iter().enumerate() {
        let edge = Edge {
            u: simplex[0],
            v: simplex[1],
        };
        if !forest.contains(&edge) {
            cotree_positions[position] = Some(cotree.len());
            cotree.push(position);
        }
    }
    let equations = triangles
        .iter()
        .map(|simplex| {
            let full = boundary_row(simplex, positions, modulus)?;
            let mut restricted = Vector::default();
            for (&position, &coefficient) in &full.0 {
                if let Some(cotree_position) = cotree_positions[position] {
                    restricted.insert(cotree_position, coefficient);
                }
            }
            Ok(restricted)
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    Ok(nullspace(equations, cotree.len(), modulus)
        .into_iter()
        .map(|row| {
            let mut expanded = Vector::default();
            for (&cotree_position, &coefficient) in &row.0 {
                expanded.insert(cotree[cotree_position], coefficient);
            }
            expanded
        })
        .collect())
}

fn spanning_forest(vertex_count: usize, adjacency: &[BTreeSet<usize>]) -> BTreeSet<Edge> {
    let mut forest = BTreeSet::new();
    let mut seen = vec![false; vertex_count];
    let mut queue = VecDeque::new();
    for root in 0..vertex_count {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        queue.push_back(root);
        while let Some(u) = queue.pop_front() {
            for &v in &adjacency[u] {
                if seen[v] {
                    continue;
                }
                seen[v] = true;
                queue.push_back(v);
                forest.insert(Edge {
                    u: u.min(v),
                    v: u.max(v),
                });
            }
        }
    }
    forest
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

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_edges(vertex_count: usize, mask: usize) -> Vec<Edge> {
        let mut edges = Vec::new();
        let mut bit = 0;
        for u in 0..vertex_count {
            for v in u + 1..vertex_count {
                if mask & (1 << bit) != 0 {
                    edges.push(Edge { u, v });
                }
                bit += 1;
            }
        }
        edges
    }

    fn rows(vectors: &[Vector]) -> Vec<BTreeMap<usize, u32>> {
        vectors.iter().map(|vector| vector.0.clone()).collect()
    }

    fn basis_terms(space: &Space, vector: &Vector) -> Vec<(Edge, u32)> {
        vector
            .0
            .iter()
            .map(|(&position, &coefficient)| {
                (
                    Edge {
                        u: space.simplices[position][0],
                        v: space.simplices[position][1],
                    },
                    coefficient,
                )
            })
            .collect()
    }

    fn shifted_terms(
        space: &Space,
        vector: &Vector,
        vertex_values: &[u32],
        modulus: u32,
    ) -> Vec<(Edge, u32)> {
        let modulus = u64::from(modulus);
        space
            .simplices
            .iter()
            .enumerate()
            .filter_map(|(position, simplex)| {
                let coefficient = u64::from(vector.0.get(&position).copied().unwrap_or(0));
                let source = u64::from(vertex_values[simplex[0]]) % modulus;
                let target = u64::from(vertex_values[simplex[1]]) % modulus;
                let coboundary = (target + modulus - source) % modulus;
                let coefficient = ((coefficient + coboundary) % modulus) as u32;
                (coefficient != 0).then_some((
                    Edge {
                        u: simplex[0],
                        v: simplex[1],
                    },
                    coefficient,
                ))
            })
            .collect()
    }

    fn assert_space_matches_unrestricted(
        vertex_count: usize,
        edges: &[Edge],
        modulus: u32,
        limits: ProofLimits,
    ) -> (Space, Space) {
        let gauged = Space::build(vertex_count, 1, edges, modulus, limits).unwrap();
        let unrestricted =
            Space::build_unrestricted(vertex_count, 1, edges, modulus, limits).unwrap();
        assert_eq!(
            gauged.simplices, unrestricted.simplices,
            "simplex order differs for n={vertex_count}, p={modulus}"
        );
        assert_eq!(
            rows(&gauged.coboundaries),
            rows(&unrestricted.coboundaries),
            "coboundaries differ for n={vertex_count}, p={modulus}"
        );
        assert_eq!(
            rows(&gauged.basis),
            rows(&unrestricted.basis),
            "basis differs for n={vertex_count}, p={modulus}"
        );
        assert_eq!(
            gauged.id(vertex_count, 1, 1.0, modulus, edges),
            unrestricted.id(vertex_count, 1, 1.0, modulus, edges),
            "space id differs for n={vertex_count}, p={modulus}"
        );
        for vector in &gauged.basis {
            let terms = basis_terms(&gauged, vector);
            assert_eq!(
                gauged.coordinates_of_edge_cocycle(&terms, modulus),
                unrestricted.coordinates_of_edge_cocycle(&terms, modulus),
                "class coordinates differ for n={vertex_count}, p={modulus}"
            );
        }
        (gauged, unrestricted)
    }

    #[test]
    fn h1_forest_gauge_matches_the_unrestricted_nullspace() {
        let limits = ProofLimits::default();
        for vertex_count in 0usize..=5 {
            let edge_bits = vertex_count * vertex_count.saturating_sub(1) / 2;
            for mask in 0..(1usize << edge_bits) {
                let edges = graph_edges(vertex_count, mask);
                for modulus in [2, 3, 5, 47] {
                    assert_space_matches_unrestricted(vertex_count, &edges, modulus, limits);
                }
            }
        }
    }

    #[test]
    fn h1_forest_gauge_preserves_disconnected_cycle_coboundary_coordinates() {
        let edges = vec![
            Edge { u: 0, v: 1 },
            Edge { u: 0, v: 3 },
            Edge { u: 1, v: 2 },
            Edge { u: 2, v: 3 },
            Edge { u: 4, v: 5 },
            Edge { u: 4, v: 7 },
            Edge { u: 5, v: 6 },
            Edge { u: 6, v: 7 },
        ];
        let (gauged, unrestricted) =
            assert_space_matches_unrestricted(8, &edges, 47, ProofLimits::default());
        assert_eq!(gauged.rank(), 2);
        let base = &gauged.basis[0];
        let base_terms = basis_terms(&gauged, base);
        let shifted = shifted_terms(&gauged, base, &[1, 0, 0, 0, 2, 0, 0, 0], 47);
        assert_ne!(base_terms, shifted);
        let gauged_base = gauged.coordinates_of_edge_cocycle(&base_terms, 47);
        assert_eq!(
            gauged_base,
            gauged.coordinates_of_edge_cocycle(&shifted, 47)
        );
        assert_eq!(
            gauged.coordinates_of_edge_cocycle(&shifted, 47),
            unrestricted.coordinates_of_edge_cocycle(&shifted, 47)
        );
    }
}
