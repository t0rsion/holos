use std::collections::{BTreeMap, VecDeque};

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::combinadic::BinomialTable;
use crate::reduce::{RawH1Class, RawH1Term};
use crate::{Bar, Error, Result, SparseDistanceMatrix};

use super::model::{
    BasisClassId, Cocycle, CocycleTerm, CriticalPair, CriticalSimplex, IntervalGroupId,
    PersistentClass, PersistentClassProvenance, PersistentClassSpace,
};
use super::validation::{coefficient, oriented_coefficient, validate_h1_cocycle};

pub(super) fn canonical_edge(u: usize, v: usize) -> (usize, usize) {
    if u < v { (u, v) } else { (v, u) }
}

pub(crate) fn canonical_spaces(
    matrix: &SparseDistanceMatrix,
    modulus: u32,
    raw: Vec<RawH1Class>,
) -> Result<Vec<PersistentClassSpace>> {
    let table = BinomialTable::new(matrix.len(), 3)?;
    let mut seeds = Vec::with_capacity(raw.len());
    for class in raw {
        let terms = canonical_terms(&table, matrix.len(), modulus, class.scale, &class.terms)?;
        let cocycle = Cocycle {
            modulus,
            scale: class.scale,
            terms,
        };
        validate_h1_cocycle(matrix, &cocycle)?;
        let critical = CriticalPair {
            birth: decode_critical(&table, matrix.len(), 1, class.birth),
            death: class
                .death
                .map(|death| decode_critical(&table, matrix.len(), 2, death)),
        };
        seeds.push((class.bar, cocycle, critical));
    }
    seeds.sort_by(|a, b| {
        a.0.birth
            .total_cmp(&b.0.birth)
            .then(a.0.death.total_cmp(&b.0.death))
            .then_with(|| critical_pair_order(&a.2, &b.2))
    });
    let mut spaces = Vec::new();
    let mut start = 0;
    while start < seeds.len() {
        let interval = seeds[start].0;
        let mut end = start + 1;
        while end < seeds.len() && bar_bits_equal(seeds[end].0, interval) {
            end += 1;
        }
        let cocycles: Vec<_> = seeds[start..end]
            .iter()
            .map(|(_, cocycle, _)| cocycle.clone())
            .collect();
        let mut critical_pairs: Vec<_> = seeds[start..end]
            .iter()
            .map(|(_, _, critical)| critical.clone())
            .collect();
        critical_pairs.sort_by(critical_pair_order);
        let basis_cocycles = canonical_space_basis(matrix, modulus, &cocycles)?;
        if basis_cocycles.len() != end - start {
            return Err(Error::InvalidInput(format!(
                "H1 interval group has multiplicity {} but class-space rank {}",
                end - start,
                basis_cocycles.len()
            )));
        }
        let id = group_id(interval, modulus, &basis_cocycles);
        let basis = basis_cocycles
            .into_iter()
            .enumerate()
            .map(|(basis_index, cocycle)| {
                let class_id = basis_class_id(id, basis_index, &cocycle);
                let provenance = PersistentClassProvenance::new(
                    matrix,
                    *class_id.as_bytes(),
                    interval,
                    &cocycle,
                );
                PersistentClass {
                    id: class_id,
                    group_id: id,
                    basis_index,
                    interval,
                    cocycle,
                    provenance: Some(provenance),
                }
            })
            .collect();
        spaces.push(PersistentClassSpace {
            id,
            interval,
            basis,
            critical_pairs,
        });
        start = end;
    }
    spaces.sort_by(|a, b| {
        a.interval
            .birth
            .total_cmp(&b.interval.birth)
            .then(a.interval.death.total_cmp(&b.interval.death))
            .then(a.id.cmp(&b.id))
    });
    Ok(spaces)
}

fn decode_critical(
    table: &BinomialTable,
    n: usize,
    dim: usize,
    simplex: crate::simplex::Simplex,
) -> CriticalSimplex {
    let mut vertices = Vec::new();
    table.unrank(simplex.index, dim, n, &mut vertices);
    CriticalSimplex {
        vertices,
        value: simplex.diameter,
    }
}

fn critical_pair_order(a: &CriticalPair, b: &CriticalPair) -> std::cmp::Ordering {
    a.birth
        .vertices
        .cmp(&b.birth.vertices)
        .then_with(|| match (&a.death, &b.death) {
            (Some(a), Some(b)) => a.vertices.cmp(&b.vertices),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

fn bar_bits_equal(a: Bar, b: Bar) -> bool {
    a.dim == b.dim
        && a.birth.to_bits() == b.birth.to_bits()
        && a.death.to_bits() == b.death.to_bits()
}

fn canonical_terms(
    table: &BinomialTable,
    n: usize,
    modulus: u32,
    scale: f64,
    raw: &[RawH1Term],
) -> Result<Vec<CocycleTerm>> {
    let modulus64 = modulus as u64;
    let mut by_edge: FxHashMap<(usize, usize), u64> = FxHashMap::default();
    let mut vertices = Vec::new();
    for term in raw.iter().filter(|term| term.simplex.diameter <= scale) {
        table.unrank(term.simplex.index, 1, n, &mut vertices);
        let edge = (vertices[0], vertices[1]);
        let slot = by_edge.entry(edge).or_insert(0);
        *slot = (*slot + term.coefficient) % modulus64;
    }
    normalize_map(by_edge, modulus64)
}

pub(super) fn normalize_map(
    by_edge: FxHashMap<(usize, usize), u64>,
    modulus: u64,
) -> Result<Vec<CocycleTerm>> {
    let mut terms: Vec<CocycleTerm> = by_edge
        .into_iter()
        .filter_map(|((u, v), coefficient)| {
            let coefficient = coefficient % modulus;
            (coefficient != 0).then_some(CocycleTerm {
                u,
                v,
                coefficient: coefficient as u32,
            })
        })
        .collect();
    terms.sort_unstable();
    let Some(first) = terms.first() else {
        return Err(Error::InvalidInput(
            "H1 reduction produced an empty representative".into(),
        ));
    };
    let inverse = inverse_mod(first.coefficient as u64, modulus);
    for term in &mut terms {
        term.coefficient = ((term.coefficient as u64 * inverse) % modulus) as u32;
    }
    Ok(terms)
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}

pub(crate) fn canonical_space_basis(
    matrix: &SparseDistanceMatrix,
    modulus: u32,
    cocycles: &[Cocycle],
) -> Result<Vec<Cocycle>> {
    let Some(first) = cocycles.first() else {
        return Ok(Vec::new());
    };
    if cocycles.iter().any(|cocycle| {
        cocycle.modulus != modulus || cocycle.scale.to_bits() != first.scale.to_bits()
    }) {
        return Err(Error::InvalidInput(
            "class-space cocycles do not share a field and scale".into(),
        ));
    }
    let active_edges: Vec<_> = matrix
        .edges()
        .filter(|&(_, _, value)| value <= first.scale)
        .map(|(u, v, _)| (u, v))
        .collect();
    let edge_index: FxHashMap<_, _> = active_edges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, edge)| (edge, index))
        .collect();
    let mut rows: BTreeMap<usize, BTreeMap<usize, u64>> = BTreeMap::new();
    for cocycle in cocycles {
        let mut row = gauge_fixed_row(
            matrix.len(),
            &active_edges,
            &edge_index,
            cocycle,
            modulus as u64,
        )?;
        for (&pivot, existing) in &rows {
            if let Some(&coefficient) = row.get(&pivot) {
                add_scaled_row(
                    &mut row,
                    existing,
                    modulus as u64 - coefficient,
                    modulus as u64,
                );
            }
        }
        let Some((&pivot, &coefficient)) = row.first_key_value() else {
            return Err(Error::InvalidInput(
                "class-space basis contains a coboundary".into(),
            ));
        };
        let inverse = inverse_mod(coefficient, modulus as u64);
        scale_row(&mut row, inverse, modulus as u64);
        for existing in rows.values_mut() {
            if let Some(&factor) = existing.get(&pivot) {
                add_scaled_row(existing, &row, modulus as u64 - factor, modulus as u64);
            }
        }
        rows.insert(pivot, row);
    }
    let basis = rows
        .into_values()
        .map(|row| Cocycle {
            modulus,
            scale: first.scale,
            terms: row
                .into_iter()
                .map(|(edge, coefficient)| {
                    let (u, v) = active_edges[edge];
                    CocycleTerm {
                        u,
                        v,
                        coefficient: coefficient as u32,
                    }
                })
                .collect(),
        })
        .collect();
    Ok(basis)
}

fn gauge_fixed_row(
    vertex_count: usize,
    active_edges: &[(usize, usize)],
    edge_index: &FxHashMap<(usize, usize), usize>,
    cocycle: &Cocycle,
    modulus: u64,
) -> Result<BTreeMap<usize, u64>> {
    let coefficients: FxHashMap<_, _> = cocycle
        .terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient as u64))
        .collect();
    let adjacency = adjacency_from_edges(vertex_count, active_edges);
    let potential = gauge_potential(&adjacency, &coefficients, modulus);
    adjusted_edge_row(active_edges, edge_index, &coefficients, &potential, modulus)
}

fn adjacency_from_edges(vertex_count: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for &(u, v) in edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    for row in &mut adjacency {
        row.sort_unstable();
    }
    adjacency
}

fn gauge_potential(
    adjacency: &[Vec<usize>],
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
) -> Vec<Option<u64>> {
    let vertex_count = adjacency.len();
    let mut potential = vec![None; vertex_count];
    let mut queue = VecDeque::new();
    for root in 0..vertex_count {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0u64);
        queue.push_back(root);
        while let Some(u) = queue.pop_front() {
            let base = potential[u].expect("queued vertex has a potential");
            for &v in &adjacency[u] {
                if potential[v].is_none() {
                    potential[v] =
                        Some((base + oriented_coefficient(coefficients, u, v, modulus)) % modulus);
                    queue.push_back(v);
                }
            }
        }
    }
    potential
}

fn adjusted_edge_row(
    active_edges: &[(usize, usize)],
    edge_index: &FxHashMap<(usize, usize), usize>,
    coefficients: &FxHashMap<(usize, usize), u64>,
    potential: &[Option<u64>],
    modulus: u64,
) -> Result<BTreeMap<usize, u64>> {
    let mut row = BTreeMap::new();
    for &(u, v) in active_edges {
        let original = coefficient(coefficients, u, v);
        let adjusted =
            (original + potential[u].unwrap_or(0) + modulus - potential[v].unwrap_or(0)) % modulus;
        if adjusted != 0 {
            let index = edge_index.get(&(u, v)).copied().ok_or_else(|| {
                Error::InvalidInput(format!("active edge ({u}, {v}) has no canonical index"))
            })?;
            row.insert(index, adjusted);
        }
    }
    Ok(row)
}

fn scale_row(row: &mut BTreeMap<usize, u64>, factor: u64, modulus: u64) {
    for value in row.values_mut() {
        *value = *value * factor % modulus;
    }
}

fn add_scaled_row(
    target: &mut BTreeMap<usize, u64>,
    source: &BTreeMap<usize, u64>,
    factor: u64,
    modulus: u64,
) {
    if factor == 0 {
        return;
    }
    for (&index, &value) in source {
        let next = (target.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
        if next == 0 {
            target.remove(&index);
        } else {
            target.insert(index, next);
        }
    }
}

pub(super) fn recanonicalize_space(
    matrix: &SparseDistanceMatrix,
    space: &mut PersistentClassSpace,
) -> Result<()> {
    let modulus = space
        .basis
        .first()
        .map(|class| class.cocycle.modulus)
        .ok_or_else(|| Error::InvalidInput("class space has no basis".into()))?;
    let cocycles: Vec<_> = space
        .basis
        .iter()
        .map(|class| class.cocycle.clone())
        .collect();
    let cocycles = canonical_space_basis(matrix, modulus, &cocycles)?;
    let id = group_id(space.interval, modulus, &cocycles);
    space.id = id;
    space.basis = cocycles
        .into_iter()
        .enumerate()
        .map(|(basis_index, cocycle)| {
            let class_id = basis_class_id(id, basis_index, &cocycle);
            let provenance = PersistentClassProvenance::new(
                matrix,
                *class_id.as_bytes(),
                space.interval,
                &cocycle,
            );
            PersistentClass {
                id: class_id,
                group_id: id,
                basis_index,
                interval: space.interval,
                cocycle,
                provenance: Some(provenance),
            }
        })
        .collect();
    Ok(())
}

pub(crate) fn group_id(interval: Bar, modulus: u32, basis: &[Cocycle]) -> IntervalGroupId {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-class-space-v1");
    hash.update((interval.dim as u64).to_be_bytes());
    hash.update(interval.birth.to_bits().to_be_bytes());
    hash.update(interval.death.to_bits().to_be_bytes());
    hash.update(modulus.to_be_bytes());
    hash.update((basis.len() as u64).to_be_bytes());
    for cocycle in basis {
        hash.update(cocycle.scale.to_bits().to_be_bytes());
        hash.update((cocycle.terms.len() as u64).to_be_bytes());
        for term in &cocycle.terms {
            hash.update((term.u as u64).to_be_bytes());
            hash.update((term.v as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
    IntervalGroupId::from_bytes(hash.finalize().into())
}

pub(crate) fn basis_class_id(
    group: IntervalGroupId,
    basis_index: usize,
    cocycle: &Cocycle,
) -> BasisClassId {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-basis-class-v1");
    hash.update(group.as_bytes());
    hash.update((basis_index as u64).to_be_bytes());
    hash.update(cocycle.scale.to_bits().to_be_bytes());
    for term in &cocycle.terms {
        hash.update((term.u as u64).to_be_bytes());
        hash.update((term.v as u64).to_be_bytes());
        hash.update(term.coefficient.to_be_bytes());
    }
    BasisClassId::from_bytes(hash.finalize().into())
}
