//! Filtered edge and triangle complexes for certificate reduction.

use rustc_hash::FxHashMap;

use crate::SparseDistanceMatrix;

use super::super::model::{CertificateError, CertificateLimits, CertificateResult};
use super::super::verify::{edge_rank, triangle_rank};
use super::columns::SparseColumn;

#[derive(Debug, Clone, Copy)]
pub(crate) struct FilteredEdge {
    pub(crate) vertices: [usize; 2],
    pub(crate) value: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FilteredTriangle {
    pub(crate) vertices: [usize; 3],
    pub(crate) value: f64,
}

pub(crate) struct FilteredComplex {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<FilteredEdge>,
    pub(crate) triangles: Vec<FilteredTriangle>,
    pub(crate) edge_rows: FxHashMap<(usize, usize), usize>,
}

impl FilteredComplex {
    pub(crate) fn build(
        input: &SparseDistanceMatrix,
        threshold: f64,
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        let edges = filtered_edges(input, threshold, limits.max_edges)?;
        let edge_rows: FxHashMap<_, _> = edges
            .iter()
            .enumerate()
            .map(|(index, edge)| ((edge.vertices[0], edge.vertices[1]), index))
            .collect();
        let triangles = filtered_triangles(input, &edges, limits.max_triangles)?;
        Ok(Self {
            vertex_count: input.len(),
            edges,
            triangles,
            edge_rows,
        })
    }

    pub(crate) fn edge_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
        let modulus = modulus as u64;
        self.edges
            .iter()
            .map(|edge| {
                let mut column = SparseColumn::default();
                column.insert(edge.vertices[0], modulus - 1);
                column.insert(edge.vertices[1], 1);
                column
            })
            .collect()
    }

    pub(crate) fn triangle_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
        let modulus = modulus as u64;
        self.triangles
            .iter()
            .map(|triangle| {
                let [u, v, w] = triangle.vertices;
                let mut column = SparseColumn::default();
                column.insert(self.edge_rows[&(v, w)], 1);
                column.insert(self.edge_rows[&(u, w)], modulus - 1);
                column.insert(self.edge_rows[&(u, v)], 1);
                column
            })
            .collect()
    }
}

fn filtered_edges(
    input: &SparseDistanceMatrix,
    threshold: f64,
    maximum: usize,
) -> CertificateResult<Vec<FilteredEdge>> {
    let mut edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .map(|(u, v, value)| FilteredEdge {
            vertices: [u, v],
            value,
        })
        .collect();
    if edges.len() > maximum {
        return Err(CertificateError::new(format!(
            "{} filtered edges exceed the limit {maximum}",
            edges.len()
        )));
    }
    edges.sort_by(|a, b| {
        a.value
            .total_cmp(&b.value)
            .then_with(|| edge_rank(b.vertices).cmp(&edge_rank(a.vertices)))
    });
    Ok(edges)
}

fn upper_adjacency(vertex_count: usize, edges: &[FilteredEdge]) -> Vec<Vec<usize>> {
    let mut upper = vec![Vec::new(); vertex_count];
    for edge in edges {
        upper[edge.vertices[0]].push(edge.vertices[1]);
    }
    for neighbors in &mut upper {
        neighbors.sort_unstable();
    }
    upper
}

fn filtered_triangles(
    input: &SparseDistanceMatrix,
    edges: &[FilteredEdge],
    maximum: usize,
) -> CertificateResult<Vec<FilteredTriangle>> {
    let upper = upper_adjacency(input.len(), edges);
    let mut triangles = Vec::new();
    for u in 0..input.len() {
        for &v in &upper[u] {
            append_edge_triangles(input, &upper, u, v, maximum, &mut triangles)?;
        }
    }
    triangles.sort_by(|a, b| {
        a.value
            .total_cmp(&b.value)
            .then_with(|| triangle_rank(b.vertices).cmp(&triangle_rank(a.vertices)))
    });
    Ok(triangles)
}

fn append_edge_triangles(
    input: &SparseDistanceMatrix,
    upper: &[Vec<usize>],
    u: usize,
    v: usize,
    maximum: usize,
    triangles: &mut Vec<FilteredTriangle>,
) -> CertificateResult<()> {
    let mut a = upper[u].partition_point(|&w| w <= v);
    let mut b = upper[v].partition_point(|&w| w <= v);
    while a < upper[u].len() && b < upper[v].len() {
        match upper[u][a].cmp(&upper[v][b]) {
            std::cmp::Ordering::Less => a += 1,
            std::cmp::Ordering::Greater => b += 1,
            std::cmp::Ordering::Equal => {
                append_triangle(input, u, v, upper[u][a], maximum, triangles)?;
                a += 1;
                b += 1;
            }
        }
    }
    Ok(())
}

fn append_triangle(
    input: &SparseDistanceMatrix,
    u: usize,
    v: usize,
    w: usize,
    maximum: usize,
    triangles: &mut Vec<FilteredTriangle>,
) -> CertificateResult<()> {
    triangles.push(FilteredTriangle {
        vertices: [u, v, w],
        value: input.get(u, v).max(input.get(u, w)).max(input.get(v, w)),
    });
    if triangles.len() > maximum {
        return Err(CertificateError::new(format!(
            "triangle count exceeds the limit {maximum}"
        )));
    }
    Ok(())
}
