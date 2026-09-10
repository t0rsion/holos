use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::classes::basis_class_id;
use crate::{
    Bar, Cocycle, CriticalPair, CriticalSimplex, Diagram, Error, IntervalGroupId, PersistentClass,
    PersistentClassSpace, Result, SparseDistanceMatrix,
};

use super::model::{EdgeKey, EndpointFormula, EvaluatedClassSpace, LineageId, SpaceFormula};

pub(crate) fn edge_values(matrix: &SparseDistanceMatrix) -> FxHashMap<EdgeKey, f64> {
    matrix
        .edges()
        .map(|(u, v, value)| (EdgeKey::new(u, v), value))
        .collect()
}

pub(crate) fn checked_threshold(threshold: Option<f64>) -> Result<f64> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(Error::InvalidInput(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

pub(crate) fn atlas_digest(
    vertex_count: usize,
    threshold: Option<f64>,
    topology: &[EdgeKey],
    values: &FxHashMap<EdgeKey, f64>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-persistence-atlas-v1");
    hash.update((vertex_count as u64).to_be_bytes());
    hash.update(
        threshold
            .map(f64::to_bits)
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update((topology.len() as u64).to_be_bytes());
    for edge in topology {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(values[edge].to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(crate) fn lineage_id(
    input_digest: [u8; 32],
    index: usize,
    space: &PersistentClassSpace,
) -> LineageId {
    let mut hash = Sha256::new();
    hash.update(b"holos-class-lineage-v1");
    hash.update(input_digest);
    hash.update((index as u64).to_be_bytes());
    hash.update(space.id.as_bytes());
    LineageId(hash.finalize().into())
}

pub(crate) fn space_formula(
    matrix: &SparseDistanceMatrix,
    digest: [u8; 32],
    index: usize,
    space: &PersistentClassSpace,
) -> Result<SpaceFormula> {
    let mut births = Vec::new();
    let mut deaths = Vec::new();
    for pair in &space.critical_pairs {
        births.extend(critical_sources(matrix, &pair.birth)?);
        if let Some(death) = &pair.death {
            deaths.extend(critical_sources(matrix, death)?);
        }
    }
    births.sort_unstable();
    births.dedup();
    deaths.sort_unstable();
    deaths.dedup();
    if births.is_empty() {
        return Err(Error::InvalidInput(
            "class space has no birth-edge provenance".into(),
        ));
    }
    if space.interval.is_essential() && !deaths.is_empty() {
        return Err(Error::InvalidInput(
            "essential class space has death provenance".into(),
        ));
    }
    if !space.interval.is_essential() && deaths.is_empty() {
        return Err(Error::InvalidInput(
            "finite class space has no death-edge provenance".into(),
        ));
    }
    Ok(SpaceFormula {
        lineage: lineage_id(digest, index, space),
        birth: EndpointFormula { sources: births },
        death: (!space.interval.is_essential()).then_some(EndpointFormula { sources: deaths }),
    })
}

pub(crate) fn critical_sources(
    matrix: &SparseDistanceMatrix,
    simplex: &CriticalSimplex,
) -> Result<Vec<EdgeKey>> {
    let mut sources = Vec::new();
    for right in 1..simplex.vertices.len() {
        for left in 0..right {
            let edge = EdgeKey::new(simplex.vertices[left], simplex.vertices[right]);
            let value = matrix.get(edge.u, edge.v);
            if value.to_bits() == simplex.value.to_bits() {
                sources.push(edge);
            }
        }
    }
    if sources.is_empty() {
        return Err(Error::InvalidInput(
            "critical simplex has no edge at its filtration value".into(),
        ));
    }
    Ok(sources)
}

pub(crate) fn evaluated_basis(
    id: IntervalGroupId,
    interval: Bar,
    cocycles: Vec<Cocycle>,
) -> Vec<PersistentClass> {
    cocycles
        .into_iter()
        .enumerate()
        .map(|(basis_index, cocycle)| PersistentClass {
            id: basis_class_id(id, basis_index, &cocycle),
            group_id: id,
            basis_index,
            interval,
            cocycle,
            provenance: None,
        })
        .collect()
}

pub(crate) fn add_space_bars(diagram: &mut Diagram, spaces: &[EvaluatedClassSpace]) {
    for space in spaces {
        diagram.bars.extend(std::iter::repeat_n(
            space.space.interval,
            space.space.basis.len(),
        ));
    }
}

pub(crate) fn evaluate_critical_pair(
    pair: &CriticalPair,
    topology: &[EdgeKey],
    values: &[f64],
) -> Result<CriticalPair> {
    Ok(CriticalPair {
        birth: evaluate_critical(&pair.birth, topology, values)?,
        death: pair
            .death
            .as_ref()
            .map(|death| evaluate_critical(death, topology, values))
            .transpose()?,
    })
}

pub(crate) fn evaluate_critical(
    simplex: &CriticalSimplex,
    topology: &[EdgeKey],
    values: &[f64],
) -> Result<CriticalSimplex> {
    let mut value = 0.0f64;
    for right in 1..simplex.vertices.len() {
        for left in 0..right {
            let edge = EdgeKey::new(simplex.vertices[left], simplex.vertices[right]);
            let position = topology.binary_search(&edge).map_err(|_| {
                Error::InvalidInput(format!(
                    "critical simplex edge ({}, {}) is absent",
                    edge.u, edge.v
                ))
            })?;
            value = value.max(values[position]);
        }
    }
    Ok(CriticalSimplex {
        vertices: simplex.vertices.clone(),
        value,
    })
}

pub(crate) fn h0_provenance(
    matrix: &SparseDistanceMatrix,
    threshold: f64,
) -> (Vec<EdgeKey>, usize) {
    let mut edges: Vec<_> = matrix
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .map(|(u, v, value)| (value, EdgeKey::new(u, v)))
        .collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut parent: Vec<_> = (0..matrix.len()).collect();
    let mut deaths = Vec::new();
    for (_, edge) in edges {
        let a = dsu_find(&mut parent, edge.u);
        let b = dsu_find(&mut parent, edge.v);
        if a != b {
            parent[b] = a;
            deaths.push(edge);
        }
    }
    let essential = (0..matrix.len())
        .filter(|&vertex| dsu_find(&mut parent, vertex) == vertex)
        .count();
    (deaths, essential)
}

pub(crate) fn dsu_find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != vertex {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

pub(crate) fn previous_float(value: f64) -> f64 {
    debug_assert!(value > 0.0 && value.is_finite());
    f64::from_bits(value.to_bits() - 1)
}

pub(crate) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}
