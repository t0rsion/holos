use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::ProofError;
use crate::proof::{Graph, ProofBar, SparseColumn, check_matrix, diagrams_equal};

use super::claim::ReductionClaim;
use super::model::ProgramProofLimits;

/// One H1 interval and the simplices that create and destroy it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct H1Pair {
    pub(super) interval: ProofBar,
    pub(super) birth: [usize; 2],
    pub(super) death: Option<[usize; 3]>,
}

/// Reduction result needed by the atlas verifier.
#[derive(Debug, Clone)]
pub(super) struct CheckedReduction {
    pub(super) h1_pairs: Vec<H1Pair>,
    pub(super) edge_columns: usize,
    pub(super) triangle_columns: usize,
}

#[derive(Clone, Copy)]
pub(super) struct FilteredEdge {
    pub(super) vertices: [usize; 2],
    pub(super) value: f64,
}

#[derive(Clone, Copy)]
pub(super) struct FilteredTriangle {
    pub(super) vertices: [usize; 3],
    pub(super) value: f64,
}

pub(super) struct Complex {
    pub(super) vertex_count: usize,
    pub(super) edges: Vec<FilteredEdge>,
    pub(super) triangles: Vec<FilteredTriangle>,
    edge_rows: BTreeMap<(usize, usize), usize>,
}

impl Complex {
    pub(super) fn build(
        graph: &Graph,
        threshold: Option<f64>,
        max_edges: usize,
        max_triangles: usize,
    ) -> Result<Self, ProofError> {
        let threshold = threshold.unwrap_or(f64::INFINITY);
        if threshold.is_nan() || threshold < 0.0 {
            return Err(ProofError::new("threshold must be non-negative"));
        }
        let edges = build_edges(graph, threshold, max_edges)?;
        let edge_rows = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.vertices[0], edge.vertices[1]), position))
            .collect();
        let triangles = build_triangles(graph, &edges, max_triangles)?;
        Ok(Self {
            vertex_count: graph.vertex_count,
            edges,
            triangles,
            edge_rows,
        })
    }

    pub(super) fn edge_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
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

    pub(super) fn triangle_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
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

fn build_edges(
    graph: &Graph,
    threshold: f64,
    max_edges: usize,
) -> Result<Vec<FilteredEdge>, ProofError> {
    let mut edges = Vec::new();
    for edge in graph.edges.iter().filter(|edge| edge.value <= threshold) {
        if edges.len() == max_edges {
            return Err(ProofError::new("filtered edge count exceeds the limit"));
        }
        edges.push(FilteredEdge {
            vertices: [edge.u, edge.v],
            value: edge.value,
        });
    }
    edges.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| edge_rank(right.vertices).cmp(&edge_rank(left.vertices)))
    });
    Ok(edges)
}

fn build_triangles(
    graph: &Graph,
    edges: &[FilteredEdge],
    max_triangles: usize,
) -> Result<Vec<FilteredTriangle>, ProofError> {
    let upper = upper_neighbors(graph.vertex_count, edges);
    let mut triangles = Vec::new();
    for u in 0..graph.vertex_count {
        for &v in &upper[u] {
            let mut left = upper[u].partition_point(|&vertex| vertex <= v);
            let mut right = upper[v].partition_point(|&vertex| vertex <= v);
            while left < upper[u].len() && right < upper[v].len() {
                match upper[u][left].cmp(&upper[v][right]) {
                    std::cmp::Ordering::Less => left += 1,
                    std::cmp::Ordering::Greater => right += 1,
                    std::cmp::Ordering::Equal => {
                        let w = upper[u][left];
                        if triangles.len() == max_triangles {
                            return Err(ProofError::new(
                                "filtered triangle count exceeds the limit",
                            ));
                        }
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
    }
    triangles.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| triangle_rank(right.vertices).cmp(&triangle_rank(left.vertices)))
    });
    Ok(triangles)
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

pub(super) fn verify_reduction(
    graph: &Graph,
    claim: &ReductionClaim,
    limits: ProgramProofLimits,
) -> Result<CheckedReduction, ProofError> {
    if claim.vertex_count != graph.vertex_count {
        return Err(ProofError::new(
            "reduction vertex count differs from its graph",
        ));
    }
    if graph_digest(graph, claim.threshold) != claim.graph_digest {
        return Err(ProofError::new("reduction graph binding does not match"));
    }
    let complex = Complex::build(
        graph,
        claim.threshold,
        limits.max_edges,
        limits.max_triangles,
    )?;
    let reduced_edges = check_matrix(
        &complex.edge_boundaries(claim.modulus),
        &claim.edge_columns,
        claim.modulus,
        "edge",
    )?;
    let reduced_triangles = check_matrix(
        &complex.triangle_boundaries(claim.modulus),
        &claim.triangle_columns,
        claim.modulus,
        "triangle",
    )?;
    let (diagram, h1_pairs) = checked_diagram(
        &complex,
        &reduced_edges,
        &reduced_triangles,
        limits.max_bars,
    )?;
    if !diagrams_equal(&diagram, &claim.diagram) {
        return Err(ProofError::new(
            "recorded reduction diagram differs from the checked reduction",
        ));
    }
    Ok(CheckedReduction {
        h1_pairs,
        edge_columns: claim.edge_columns.len(),
        triangle_columns: claim.triangle_columns.len(),
    })
}

pub(super) fn checked_diagram(
    complex: &Complex,
    reduced_edges: &[SparseColumn],
    reduced_triangles: &[SparseColumn],
    max_bars: usize,
) -> Result<(Vec<ProofBar>, Vec<H1Pair>), ProofError> {
    let mut diagram = zero_dimensional_diagram(complex, reduced_edges, max_bars)?;
    let deaths = triangle_deaths(complex, reduced_triangles);
    let pairs = one_dimensional_pairs(complex, reduced_edges, &deaths, &mut diagram, max_bars)?;
    diagram.sort_by(diagram_order);
    let mut pairs = pairs;
    pairs.sort_by(h1_pair_order);
    Ok((diagram, pairs))
}

fn zero_dimensional_diagram(
    complex: &Complex,
    reduced_edges: &[SparseColumn],
    max_bars: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let mut diagram = Vec::new();
    let mut killed_vertices = vec![false; complex.vertex_count];
    for (position, reduced) in reduced_edges.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            killed_vertices[pivot] = true;
            let death = complex.edges[position].value;
            if death > 0.0 {
                push_bar(
                    &mut diagram,
                    ProofBar {
                        dimension: 0,
                        birth: 0.0,
                        death,
                    },
                    max_bars,
                )?;
            }
        }
    }
    for killed in &killed_vertices {
        if !killed {
            push_bar(
                &mut diagram,
                ProofBar {
                    dimension: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                },
                max_bars,
            )?;
        }
    }
    Ok(diagram)
}

fn triangle_deaths(
    complex: &Complex,
    reduced_triangles: &[SparseColumn],
) -> BTreeMap<usize, FilteredTriangle> {
    let mut deaths = BTreeMap::new();
    for (position, reduced) in reduced_triangles.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            deaths.insert(pivot, complex.triangles[position]);
        }
    }
    deaths
}

fn one_dimensional_pairs(
    complex: &Complex,
    reduced_edges: &[SparseColumn],
    deaths: &BTreeMap<usize, FilteredTriangle>,
    diagram: &mut Vec<ProofBar>,
    max_bars: usize,
) -> Result<Vec<H1Pair>, ProofError> {
    let mut pairs = Vec::new();
    for (edge, reduced) in reduced_edges.iter().enumerate() {
        if !reduced.0.is_empty() {
            continue;
        }
        let birth = complex.edges[edge].value;
        let death_simplex = deaths.get(&edge).copied();
        let death = death_simplex.map_or(f64::INFINITY, |simplex| simplex.value);
        if death <= birth {
            continue;
        }
        let interval = ProofBar {
            dimension: 1,
            birth,
            death,
        };
        push_bar(diagram, interval, max_bars)?;
        pairs.push(H1Pair {
            interval,
            birth: complex.edges[edge].vertices,
            death: death_simplex.map(|simplex| simplex.vertices),
        });
    }
    Ok(pairs)
}

fn diagram_order(left: &ProofBar, right: &ProofBar) -> std::cmp::Ordering {
    left.dimension
        .cmp(&right.dimension)
        .then(left.birth.total_cmp(&right.birth))
        .then(left.death.total_cmp(&right.death))
}

fn h1_pair_order(left: &H1Pair, right: &H1Pair) -> std::cmp::Ordering {
    left.interval
        .birth
        .total_cmp(&right.interval.birth)
        .then(left.interval.death.total_cmp(&right.interval.death))
        .then(left.birth.cmp(&right.birth))
        .then_with(|| match (&left.death, &right.death) {
            (Some(a), Some(b)) => a.cmp(b),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

fn push_bar(diagram: &mut Vec<ProofBar>, bar: ProofBar, max_bars: usize) -> Result<(), ProofError> {
    if diagram.len() == max_bars {
        return Err(ProofError::new("checked reduction exceeds the bar limit"));
    }
    diagram.push(bar);
    Ok(())
}

pub(super) fn graph_digest(graph: &Graph, threshold: Option<f64>) -> [u8; 32] {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((graph.vertex_count as u64).to_be_bytes());
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .collect();
    hash.update((edges.len() as u64).to_be_bytes());
    for edge in edges {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn atlas_digest(graph: &Graph, threshold: Option<f64>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-persistence-atlas-v1");
    hash.update((graph.vertex_count as u64).to_be_bytes());
    hash.update(
        threshold
            .map(f64::to_bits)
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update((graph.edges.len() as u64).to_be_bytes());
    for edge in &graph.edges {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * v.saturating_sub(1) as u128 / 2 + u as u128
}

pub(super) fn triangle_rank([u, v, w]: [usize; 3]) -> u128 {
    let choose2 = v as u128 * v.saturating_sub(1) as u128 / 2;
    let choose3 = w as u128 * w.saturating_sub(1) as u128 * w.saturating_sub(2) as u128 / 6;
    u as u128 + choose2 + choose3
}

#[cfg(test)]
mod tests {
    use super::Complex;
    use crate::proof::{Graph, ProofEdge};

    fn complete_graph() -> Graph {
        Graph::new(
            4,
            &[
                ProofEdge {
                    u: 0,
                    v: 1,
                    value: 1.0,
                },
                ProofEdge {
                    u: 0,
                    v: 2,
                    value: 1.0,
                },
                ProofEdge {
                    u: 0,
                    v: 3,
                    value: 1.0,
                },
                ProofEdge {
                    u: 1,
                    v: 2,
                    value: 1.0,
                },
                ProofEdge {
                    u: 1,
                    v: 3,
                    value: 1.0,
                },
                ProofEdge {
                    u: 2,
                    v: 3,
                    value: 1.0,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn filtered_complex_rejects_edges_before_materializing_the_reduction() {
        let graph = complete_graph();
        let error = match Complex::build(&graph, None, 5, 4) {
            Ok(_) => panic!("edge limit was not enforced"),
            Err(error) => error,
        };
        assert_eq!(error.message(), "filtered edge count exceeds the limit");
    }

    #[test]
    fn filtered_complex_rejects_triangles_before_materializing_the_reduction() {
        let graph = complete_graph();
        let error = match Complex::build(&graph, None, 6, 3) {
            Ok(_) => panic!("triangle limit was not enforced"),
            Err(error) => error,
        };
        assert_eq!(error.message(), "filtered triangle count exceeds the limit");
    }
}
