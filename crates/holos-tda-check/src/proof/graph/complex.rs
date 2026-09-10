use crate::ProofError;

use super::diagram::checked_threshold;
use super::model::{FilteredComplex, FilteredEdge, FilteredTriangle, Graph, SparseColumn};

impl FilteredComplex {
    pub(crate) fn build(graph: &Graph, threshold: Option<f64>) -> Result<Self, ProofError> {
        let threshold = checked_threshold(threshold)?;
        let edges = filtered_edges(graph, threshold);
        let edge_rows = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.vertices[0], edge.vertices[1]), position))
            .collect();
        let upper = upper_neighbors(graph.vertex_count, &edges);
        let triangles = filtered_triangles(graph, &upper);
        Ok(Self {
            vertex_count: graph.vertex_count,
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

fn filtered_edges(graph: &Graph, threshold: f64) -> Vec<FilteredEdge> {
    let mut edges = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .map(|edge| FilteredEdge {
            vertices: [edge.u, edge.v],
            value: edge.value,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| edge_rank(right.vertices).cmp(&edge_rank(left.vertices)))
    });
    edges
}

fn upper_neighbors(vertex_count: usize, edges: &[FilteredEdge]) -> Vec<Vec<usize>> {
    let mut upper = vec![Vec::new(); vertex_count];
    for edge in edges {
        upper[edge.vertices[0]].push(edge.vertices[1]);
    }
    for neighbors in &mut upper {
        neighbors.sort_unstable();
    }
    upper
}

fn filtered_triangles(graph: &Graph, upper: &[Vec<usize>]) -> Vec<FilteredTriangle> {
    let mut triangles = Vec::new();
    for u in 0..graph.vertex_count {
        for &v in &upper[u] {
            append_common_triangles(&mut triangles, graph, upper, u, v);
        }
    }
    triangles.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| triangle_rank(right.vertices).cmp(&triangle_rank(left.vertices)))
    });
    triangles
}

fn append_common_triangles(
    triangles: &mut Vec<FilteredTriangle>,
    graph: &Graph,
    upper: &[Vec<usize>],
    u: usize,
    v: usize,
) {
    let mut left = upper[u].partition_point(|&vertex| vertex <= v);
    let mut right = upper[v].partition_point(|&vertex| vertex <= v);
    while left < upper[u].len() && right < upper[v].len() {
        match upper[u][left].cmp(&upper[v][right]) {
            std::cmp::Ordering::Less => left += 1,
            std::cmp::Ordering::Greater => right += 1,
            std::cmp::Ordering::Equal => {
                let w = upper[u][left];
                triangles.push(FilteredTriangle {
                    vertices: [u, v, w],
                    value: graph.get(u, v).max(graph.get(u, w)).max(graph.get(v, w)),
                });
                left += 1;
                right += 1;
            }
        }
    }
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * v.saturating_sub(1) as u128 / 2 + u as u128
}

fn triangle_rank([u, v, w]: [usize; 3]) -> u128 {
    let choose2 = v as u128 * v.saturating_sub(1) as u128 / 2;
    let choose3 = w as u128 * w.saturating_sub(1) as u128 * w.saturating_sub(2) as u128 / 6;
    u as u128 + choose2 + choose3
}
