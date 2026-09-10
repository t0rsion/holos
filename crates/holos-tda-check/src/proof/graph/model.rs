use std::collections::BTreeMap;

use crate::ProofError;

use super::super::model::ProofEdge;

#[derive(Debug, Clone)]
pub(crate) struct Graph {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<ProofEdge>,
    pub(crate) positions: BTreeMap<(usize, usize), usize>,
}

impl Graph {
    pub(crate) fn new(vertex_count: usize, edges: &[ProofEdge]) -> Result<Self, ProofError> {
        let positions: BTreeMap<_, _> = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.u, edge.v), position))
            .collect();
        if positions.len() != edges.len() {
            return Err(ProofError::new("graph contains duplicate edges"));
        }
        Ok(Self {
            vertex_count,
            edges: edges.to_vec(),
            positions,
        })
    }

    pub(crate) fn get(&self, u: usize, v: usize) -> f64 {
        let edge = if u < v { (u, v) } else { (v, u) };
        self.positions
            .get(&edge)
            .map_or(f64::INFINITY, |&position| self.edges[position].value)
    }

    pub(crate) fn local(
        &self,
        vertices: &[usize],
        edges: &[[usize; 2]],
    ) -> Result<Self, ProofError> {
        let local_positions: BTreeMap<_, _> = vertices
            .iter()
            .copied()
            .enumerate()
            .map(|(position, vertex)| (vertex, position))
            .collect();
        let mut local_edges = Vec::with_capacity(edges.len());
        for &[u, v] in edges {
            local_edges.push(ProofEdge {
                u: local_positions[&u],
                v: local_positions[&v],
                value: self.get(u, v),
            });
        }
        local_edges.sort_by_key(|edge| (edge.u, edge.v));
        Self::new(vertices.len(), &local_edges)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub(crate) vertices: Vec<usize>,
    pub(crate) edges: Vec<[usize; 2]>,
}

#[derive(Clone, Copy)]
pub(crate) struct FilteredEdge {
    pub(crate) vertices: [usize; 2],
    pub(crate) value: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct FilteredTriangle {
    pub(crate) vertices: [usize; 3],
    pub(crate) value: f64,
}

pub(crate) struct FilteredComplex {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<FilteredEdge>,
    pub(crate) triangles: Vec<FilteredTriangle>,
    pub(crate) edge_rows: BTreeMap<(usize, usize), usize>,
}

#[derive(Clone, Default)]
pub(crate) struct SparseColumn(pub(crate) BTreeMap<usize, u64>);

impl SparseColumn {
    pub(crate) fn insert(&mut self, position: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    pub(crate) fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&position, &coefficient)| (position, coefficient))
    }

    pub(crate) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&position, &coefficient) in &source.0 {
            let next =
                (self.0.get(&position).copied().unwrap_or(0) + factor * coefficient) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next);
            }
        }
    }
}
