use crate::{Bar, CriticalPair, Diagram, EdgeKey, Error, Result, SparseDistanceMatrix};

use super::model::{PersistenceProgram, ProgramEvent, ProgramEventKind};

pub(crate) fn program_topology_events(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> Vec<ProgramEvent> {
    if updated.len() != program.graph.len() {
        return vec![ProgramEvent {
            kind: ProgramEventKind::VertexSetChanged,
            atom: None,
            edge: None,
            guard: None,
        }];
    }
    let topology: Vec<_> = updated
        .edges()
        .map(|(u, v, _)| EdgeKey::new(u, v))
        .collect();
    if topology != program.topology {
        let edge = program
            .topology
            .iter()
            .find(|edge| topology.binary_search(edge).is_err())
            .or_else(|| {
                topology
                    .iter()
                    .find(|edge| program.topology.binary_search(edge).is_err())
            })
            .copied();
        return vec![ProgramEvent {
            kind: ProgramEventKind::EdgeSetChanged,
            atom: None,
            edge,
            guard: None,
        }];
    }
    let threshold = program.params.threshold.unwrap_or(f64::INFINITY);
    let mut events: Vec<_> = program
        .topology
        .iter()
        .enumerate()
        .filter_map(|(index, &edge)| {
            ((updated.get(edge.u, edge.v) <= threshold) != program.active[index]).then_some(
                ProgramEvent {
                    kind: ProgramEventKind::ThresholdCrossing,
                    atom: None,
                    edge: Some(edge),
                    guard: None,
                },
            )
        })
        .collect();
    if events.is_empty() {
        events.extend(program.separator_edges.iter().filter_map(|&edge| {
            (updated.get(edge.u, edge.v).to_bits() != 0).then_some(ProgramEvent {
                kind: ProgramEventKind::SeparatorContractChanged,
                atom: None,
                edge: Some(edge),
                guard: None,
            })
        }));
    }
    events
}

pub(super) fn check_program_topology(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> Result<Vec<f64>> {
    if updated.len() != program.graph.len() {
        return Err(Error::InvalidInput(
            "persistence program topology ended at VertexSetChanged".into(),
        ));
    }
    if updated.num_edges() != program.topology.len() {
        return Err(Error::InvalidInput(
            "persistence program topology ended at EdgeSetChanged".into(),
        ));
    }
    let threshold = program.params.threshold.unwrap_or(f64::INFINITY);
    let mut values = Vec::with_capacity(program.topology.len());
    for ((u, v, weight), (&edge, &active)) in updated
        .edges()
        .zip(program.topology.iter().zip(&program.active))
    {
        if edge != EdgeKey::new(u, v) {
            return Err(Error::InvalidInput(
                "persistence program topology ended at EdgeSetChanged".into(),
            ));
        }
        if (weight <= threshold) != active {
            return Err(Error::InvalidInput(
                "persistence program topology ended at ThresholdCrossing".into(),
            ));
        }
        values.push(weight);
    }
    if program
        .separator_edges
        .iter()
        .any(|edge| updated.get(edge.u, edge.v).to_bits() != 0)
    {
        return Err(Error::InvalidInput(
            "persistence program topology ended at SeparatorContractChanged".into(),
        ));
    }
    Ok(values)
}

pub(super) fn h0_diagram(input: &SparseDistanceMatrix, threshold: Option<f64>) -> (Diagram, usize) {
    let (deaths, components, scanned) = h0_provenance(input, threshold);
    let mut diagram = Diagram::default();
    for edge in deaths {
        let death = input.get(edge.u, edge.v);
        if death > 0.0 {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death,
            });
        }
    }
    diagram.bars.extend((0..components).map(|_| Bar {
        dim: 0,
        birth: 0.0,
        death: f64::INFINITY,
    }));
    (diagram, scanned)
}

pub(super) fn h0_provenance(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> (Vec<EdgeKey>, usize, usize) {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    let mut edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .collect();
    edges.sort_by(|a, b| {
        a.2.total_cmp(&b.2)
            .then_with(|| edge_rank([b.0, b.1]).cmp(&edge_rank([a.0, a.1])))
    });
    let scanned = edges.len();
    let mut parent: Vec<_> = (0..input.len()).collect();
    let mut rank = vec![0u8; input.len()];
    let mut deaths = Vec::new();
    for (u, v, _) in edges {
        let left = dsu_find(&mut parent, u);
        let right = dsu_find(&mut parent, v);
        if left == right {
            continue;
        }
        dsu_link(&mut parent, &mut rank, left, right);
        deaths.push(EdgeKey::new(u, v));
    }
    let components = (0..input.len())
        .filter(|&vertex| dsu_find(&mut parent, vertex) == vertex)
        .count();
    (deaths, components, scanned)
}

fn dsu_find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != root {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

fn dsu_link(parent: &mut [usize], rank: &mut [u8], left: usize, right: usize) {
    if rank[left] < rank[right] {
        parent[left] = right;
    } else {
        parent[right] = left;
        if rank[left] == rank[right] {
            rank[left] += 1;
        }
    }
}

pub(super) fn map_critical_pair(pair: &CriticalPair, vertices: &[usize]) -> CriticalPair {
    CriticalPair {
        birth: crate::CriticalSimplex {
            vertices: pair.birth.vertices.iter().map(|&v| vertices[v]).collect(),
            value: pair.birth.value,
        },
        death: pair.death.as_ref().map(|death| crate::CriticalSimplex {
            vertices: death.vertices.iter().map(|&v| vertices[v]).collect(),
            value: death.value,
        }),
    }
}

pub(super) fn critical_pair_key(pair: &CriticalPair) -> (Vec<usize>, Option<Vec<usize>>) {
    (
        pair.birth.vertices.clone(),
        pair.death.as_ref().map(|simplex| simplex.vertices.clone()),
    )
}

pub(super) fn critical_pair_order(a: &CriticalPair, b: &CriticalPair) -> std::cmp::Ordering {
    critical_pair_key(a).cmp(&critical_pair_key(b))
}

pub(super) fn simplex_edge(simplex: &crate::FiltrationSimplex) -> Option<EdgeKey> {
    match simplex.vertices() {
        &[u, v] => Some(EdgeKey::new(u, v)),
        _ => None,
    }
}

pub(super) fn terminal_level(input: &SparseDistanceMatrix, threshold: Option<f64>) -> f64 {
    threshold.unwrap_or_else(|| {
        input
            .edges()
            .map(|(_, _, value)| value)
            .fold(0.0f64, f64::max)
    })
}

pub(super) fn previous_float(value: f64) -> f64 {
    debug_assert!(value.is_finite() && value > 0.0);
    f64::from_bits(value.to_bits() - 1)
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

pub(super) fn bar_bits_equal(a: Bar, b: Bar) -> bool {
    a.dim == b.dim
        && a.birth.to_bits() == b.birth.to_bits()
        && a.death.to_bits() == b.death.to_bits()
}

pub(crate) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}
