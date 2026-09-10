use std::collections::BTreeMap;

use crate::ProofError;
use crate::proof::{Graph, ProofBar, SparseColumn, check_matrix};

use super::claim::ReductionClaim;
use super::model::ProgramProofLimits;
use super::reduction::{Complex, H1Pair};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RegionGuardKind {
    ChangeOfBasis,
    Pivot,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RegionGuard {
    pub(super) kind: RegionGuardKind,
    pub(super) earlier: Vec<usize>,
    pub(super) later: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegionViolationKind {
    VertexSetChanged,
    EdgeSetChanged,
    ThresholdCrossing,
    GuardFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RegionViolation {
    pub(super) kind: RegionViolationKind,
    pub(super) guard: Option<RegionGuardKind>,
    pub(super) edge: Option<[usize; 2]>,
}

#[derive(Debug, Clone)]
pub(super) struct ReuseRegion {
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) topology: Vec<[usize; 2]>,
    pub(super) active: Vec<bool>,
    pub(super) guards: Vec<RegionGuard>,
    pub(super) h1_pairs: Vec<H1Pair>,
}

pub(super) fn build_reuse_region(
    graph: &Graph,
    claim: &ReductionClaim,
    limits: ProgramProofLimits,
) -> Result<ReuseRegion, ProofError> {
    let (complex, reduced_edges, reduced_triangles) =
        checked_region_reduction(graph, claim, limits)?;
    let guards = collect_region_guards(&complex, claim, &reduced_triangles);
    let guards = minimize_region_guards(guards);
    let topology = graph.edges.iter().map(|edge| [edge.u, edge.v]).collect();
    let threshold = claim.threshold.unwrap_or(f64::INFINITY);
    let active = graph
        .edges
        .iter()
        .map(|edge| edge.value <= threshold)
        .collect();
    let h1_pairs = checked_h1_pairs_for_region(&complex, &reduced_edges, &reduced_triangles);
    Ok(ReuseRegion {
        vertex_count: graph.vertex_count,
        threshold: claim.threshold,
        topology,
        active,
        guards,
        h1_pairs,
    })
}

fn checked_region_reduction(
    graph: &Graph,
    claim: &ReductionClaim,
    limits: ProgramProofLimits,
) -> Result<(Complex, Vec<SparseColumn>, Vec<SparseColumn>), ProofError> {
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
    Ok((complex, reduced_edges, reduced_triangles))
}

fn collect_region_guards(
    complex: &Complex,
    claim: &ReductionClaim,
    reduced_triangles: &[SparseColumn],
) -> BTreeMap<(Vec<usize>, Vec<usize>), RegionGuardKind> {
    let mut guards = BTreeMap::new();
    let edge_vertices = complex
        .edges
        .iter()
        .map(|edge| edge.vertices.to_vec())
        .collect::<Vec<_>>();
    collect_change_of_basis_guards(&mut guards, &claim.edge_columns, &edge_vertices);
    let triangle_vertices = complex
        .triangles
        .iter()
        .map(|triangle| triangle.vertices.to_vec())
        .collect::<Vec<_>>();
    collect_change_of_basis_guards(&mut guards, &claim.triangle_columns, &triangle_vertices);
    for column in reduced_triangles {
        let Some((pivot, _)) = column.pivot() else {
            continue;
        };
        for &row in column.0.keys() {
            if row != pivot {
                insert_region_guard(
                    &mut guards,
                    RegionGuardKind::Pivot,
                    complex.edges[row].vertices.to_vec(),
                    complex.edges[pivot].vertices.to_vec(),
                );
            }
        }
    }
    guards
}

fn collect_change_of_basis_guards(
    guards: &mut BTreeMap<(Vec<usize>, Vec<usize>), RegionGuardKind>,
    columns: &[crate::proof::ProofColumn],
    simplices: &[Vec<usize>],
) {
    for (target, column) in columns.iter().enumerate() {
        for term in &column.terms {
            if term.index != target {
                insert_region_guard(
                    guards,
                    RegionGuardKind::ChangeOfBasis,
                    simplices[term.index].clone(),
                    simplices[target].clone(),
                );
            }
        }
    }
}

fn checked_h1_pairs_for_region(
    complex: &Complex,
    reduced_edges: &[SparseColumn],
    reduced_triangles: &[SparseColumn],
) -> Vec<H1Pair> {
    let mut deaths = BTreeMap::new();
    for (column, reduced) in reduced_triangles.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            deaths.insert(pivot, complex.triangles[column].vertices);
        }
    }
    let mut pairs = Vec::new();
    for (edge, reduced) in reduced_edges.iter().enumerate() {
        if !reduced.0.is_empty() {
            continue;
        }
        let birth = complex.edges[edge].value;
        let death_simplex = deaths.get(&edge).copied();
        let death = death_simplex
            .map(|simplex| complex.graph_value(simplex))
            .unwrap_or(f64::INFINITY);
        pairs.push(H1Pair {
            interval: ProofBar {
                dimension: 1,
                birth,
                death,
            },
            birth: complex.edges[edge].vertices,
            death: death_simplex,
        });
    }
    pairs.sort_by(|left, right| {
        left.interval
            .birth
            .total_cmp(&right.interval.birth)
            .then(left.interval.death.total_cmp(&right.interval.death))
            .then(left.birth.cmp(&right.birth))
            .then_with(|| match (left.death, right.death) {
                (Some(a), Some(b)) => a.cmp(&b),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    pairs
}

impl Complex {
    fn graph_value(&self, simplex: [usize; 3]) -> f64 {
        let value = |edge: [usize; 2]| {
            self.edges
                .iter()
                .find(|candidate| candidate.vertices == edge)
                .map(|candidate| candidate.value)
                .unwrap_or(f64::INFINITY)
        };
        value([simplex[0], simplex[1]])
            .max(value([simplex[0], simplex[2]]))
            .max(value([simplex[1], simplex[2]]))
    }
}

fn insert_region_guard(
    guards: &mut BTreeMap<(Vec<usize>, Vec<usize>), RegionGuardKind>,
    kind: RegionGuardKind,
    earlier: Vec<usize>,
    later: Vec<usize>,
) {
    guards
        .entry((earlier, later))
        .and_modify(|current| *current = (*current).min(kind))
        .or_insert(kind);
}

fn minimize_region_guards(
    guards: BTreeMap<(Vec<usize>, Vec<usize>), RegionGuardKind>,
) -> Vec<RegionGuard> {
    let mut nodes = BTreeMap::new();
    for (earlier, later) in guards.keys() {
        let next = nodes.len();
        nodes.entry(earlier.clone()).or_insert(next);
        let next = nodes.len();
        nodes.entry(later.clone()).or_insert(next);
    }
    let edges: Vec<_> = guards
        .into_iter()
        .map(|((earlier, later), kind)| {
            (
                nodes[&earlier],
                nodes[&later],
                RegionGuard {
                    kind,
                    earlier,
                    later,
                },
            )
        })
        .collect();
    let mut outgoing = vec![Vec::new(); nodes.len()];
    for (source, target, _) in &edges {
        outgoing[*source].push(*target);
    }
    let mut reduced = Vec::new();
    for (source, target, guard) in edges {
        let mut seen = vec![false; nodes.len()];
        let mut stack: Vec<_> = outgoing[source]
            .iter()
            .copied()
            .filter(|&next| next != target)
            .collect();
        let mut alternate = false;
        while let Some(node) = stack.pop() {
            if node == target {
                alternate = true;
                break;
            }
            if seen[node] {
                continue;
            }
            seen[node] = true;
            stack.extend(outgoing[node].iter().copied());
        }
        if !alternate {
            reduced.push(guard);
        }
    }
    reduced.sort();
    reduced
}

impl ReuseRegion {
    pub(super) fn violations(&self, graph: &Graph) -> Vec<RegionViolation> {
        if graph.vertex_count != self.vertex_count {
            return vec![RegionViolation {
                kind: RegionViolationKind::VertexSetChanged,
                guard: None,
                edge: None,
            }];
        }
        let topology: Vec<_> = graph.edges.iter().map(|edge| [edge.u, edge.v]).collect();
        if topology != self.topology {
            let edge = self
                .topology
                .iter()
                .find(|edge| topology.binary_search(edge).is_err())
                .or_else(|| {
                    topology
                        .iter()
                        .find(|edge| self.topology.binary_search(edge).is_err())
                })
                .copied();
            return vec![RegionViolation {
                kind: RegionViolationKind::EdgeSetChanged,
                guard: None,
                edge,
            }];
        }
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        let mut violations = Vec::new();
        for (index, edge) in graph.edges.iter().enumerate() {
            if (edge.value <= threshold) != self.active[index] {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::ThresholdCrossing,
                    guard: None,
                    edge: Some([edge.u, edge.v]),
                });
            }
        }
        if !violations.is_empty() {
            return violations;
        }
        for guard in &self.guards {
            let earlier = simplex_value(graph, &guard.earlier);
            let later = simplex_value(graph, &guard.later);
            let order = earlier
                .total_cmp(&later)
                .then_with(|| simplex_rank(&guard.later).cmp(&simplex_rank(&guard.earlier)));
            if order.is_gt() {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::GuardFailed,
                    guard: Some(guard.kind),
                    edge: simplex_edge(&guard.earlier),
                });
            }
        }
        violations
    }

    pub(super) fn evaluate_h1(&self, graph: &Graph) -> Vec<H1Pair> {
        self.h1_pairs
            .iter()
            .filter_map(|pair| {
                let birth = graph.get(pair.birth[0], pair.birth[1]);
                let death = pair
                    .death
                    .map(|[u, v, w]| graph.get(u, v).max(graph.get(u, w)).max(graph.get(v, w)))
                    .unwrap_or(f64::INFINITY);
                (death > birth).then_some(H1Pair {
                    interval: ProofBar {
                        dimension: 1,
                        birth,
                        death,
                    },
                    birth: pair.birth,
                    death: pair.death,
                })
            })
            .collect()
    }
}

fn simplex_value(graph: &Graph, simplex: &[usize]) -> f64 {
    match simplex {
        [_] => 0.0,
        [u, v] => graph.get(*u, *v),
        [u, v, w] => graph
            .get(*u, *v)
            .max(graph.get(*u, *w))
            .max(graph.get(*v, *w)),
        _ => f64::INFINITY,
    }
}

fn simplex_edge(simplex: &[usize]) -> Option<[usize; 2]> {
    match simplex {
        [u, v] => Some([*u, *v]),
        _ => None,
    }
}

fn simplex_rank(simplex: &[usize]) -> u128 {
    match simplex {
        [u] => *u as u128,
        [u, v] => super::reduction::edge_rank([*u, *v]),
        [u, v, w] => super::reduction::triangle_rank([*u, *v, *w]),
        _ => u128::MAX,
    }
}
