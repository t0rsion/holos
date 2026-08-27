//! Stable H1 classes and cocycles on the caller's graph.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::combinadic::BinomialTable;
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::reduce::{RawH1Class, RawH1Term};
use crate::{
    Bar, Diagram, Error, GraphFactorization, Result, RipsParams, SparseDistanceMatrix,
    collapse::{CollapsedRips, verify::verify_sparse},
};

/// Identifier of a persistent interval group and its class space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntervalGroupId([u8; 32]);

impl IntervalGroupId {
    /// Identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for IntervalGroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Identifier of one vector in a declared canonical class-space basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BasisClassId([u8; 32]);

impl BasisClassId {
    /// Identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for BasisClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// One nonzero coefficient on an oriented edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CocycleTerm {
    /// Lower endpoint. The edge is oriented from `u` to `v`.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Canonical H1 cocycle at one filtration scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Cocycle {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Scale at which the terms represent the class.
    pub scale: f64,
    /// Nonzero terms in ascending endpoint order. The first coefficient is
    /// one.
    pub terms: Vec<CocycleTerm>,
}

/// One simplex that creates or destroys a persistence interval.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalSimplex {
    /// Vertices in ascending order.
    pub vertices: Vec<usize>,
    /// Filtration value of the simplex.
    pub value: f64,
}

/// Creator and optional destroyer of one generated interval.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalPair {
    /// Edge that creates the H1 interval.
    pub birth: CriticalSimplex,
    /// Triangle that destroys a finite H1 interval.
    pub death: Option<CriticalSimplex>,
}

/// One vector in the declared basis of a persistent H1 class space.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentClass {
    /// Identifier derived from the class space, basis position, and cocycle.
    pub id: BasisClassId,
    /// Class space that contains this basis vector.
    pub group_id: IntervalGroupId,
    /// Position in the canonical basis of the class space.
    pub basis_index: usize,
    /// H1 persistence interval.
    pub interval: Bar,
    /// Representative on the caller's graph.
    pub cocycle: Cocycle,
}

/// All H1 classes with the same interval, represented as one class space.
///
/// Equal intervals do not have intrinsic individual identities. `basis` is
/// a deterministic row-reduced basis tied to the labeled input graph.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentClassSpace {
    /// Identifier derived from the interval and canonical basis.
    pub id: IntervalGroupId,
    /// Shared H1 persistence interval.
    pub interval: Bar,
    /// Canonical basis of this class space.
    pub basis: Vec<PersistentClass>,
    /// Creator and destroyer pairs from the fixed reduction before basis
    /// canonicalization.
    pub critical_pairs: Vec<CriticalPair>,
}

/// Diagram plus canonical spaces for every positive H1 interval.
#[derive(Debug, Clone)]
pub struct ExplainedDiagram {
    /// Full persistence diagram through the requested dimension.
    pub diagram: Diagram,
    /// H1 class spaces in interval order, then identifier order.
    pub spaces: Vec<PersistentClassSpace>,
}

impl ExplainedDiagram {
    /// Number of positive H1 intervals, including multiplicity.
    pub fn class_count(&self) -> usize {
        self.spaces.iter().map(|space| space.basis.len()).sum()
    }

    /// Basis vectors from every class space in canonical order.
    pub fn classes(&self) -> impl Iterator<Item = &PersistentClass> {
        self.spaces.iter().flat_map(|space| &space.basis)
    }
}

/// Compute a diagram and stable H1 classes from a sparse matrix.
///
/// On a fixed graph, class production uses a fixed whole-graph reduction
/// profile. This keeps class identifiers independent of worker count,
/// structural routing, and optional reduction shortcuts. The ordinary
/// compute API keeps those fast paths.
pub fn rips_persistence_with_classes_sparse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<ExplainedDiagram> {
    if params.max_dim < 1 {
        return Err(Error::InvalidInput(
            "H1 classes require max_dim of at least 1".into(),
        ));
    }
    if params.collapse_edges {
        let collapsed = match params.collapse_schedule {
            crate::CollapseSchedule::Serial => {
                crate::collapse::collapse_sparse(matrix, params.threshold)?
            }
            crate::CollapseSchedule::Ordered => crate::collapse::collapse_sparse_ordered_parallel(
                matrix,
                params.threshold,
                params.threads,
            )?,
            crate::CollapseSchedule::Rounds => crate::collapse::collapse_sparse_rounds_parallel(
                matrix,
                params.threshold,
                params.threads,
            )?,
            crate::CollapseSchedule::Adaptive => crate::collapse::collapse_sparse_adaptive(
                matrix,
                params.threshold,
                params.adaptive_collapse,
            )?,
        };
        let mut inner = params.clone();
        inner.collapse_edges = false;
        inner.threshold = Some(collapsed.certificate.terminal_level());
        let explained = rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
        return lift_h1_classes(&collapsed, explained);
    }
    let mut fixed = params.clone();
    fixed.factorization = GraphFactorization::Off;
    fixed.use_emergent_pairs = false;
    fixed.use_apparent_pairs = false;
    fixed.use_adjacency_rows = false;
    fixed.use_clearing = true;
    let (diagram, raw) = crate::solver::compute_with_h1_classes(matrix, &fixed)?;
    let spaces = canonical_spaces(matrix, params.modulus, raw)?;
    let h1 = diagram.in_dim(1).count();
    let class_count: usize = spaces.iter().map(|space| space.basis.len()).sum();
    if h1 != class_count {
        return Err(Error::InvalidInput(format!(
            "H1 reduction returned {h1} intervals but {class_count} basis classes"
        )));
    }
    Ok(ExplainedDiagram { diagram, spaces })
}

/// Lift stable H1 cocycles through a checked collapse trace.
///
/// The input classes must describe `collapsed.matrix`. The returned classes
/// use the reconstructed input vertex labels and edges. The function checks
/// the collapse certificate independently, then checks every lifted cocycle
/// on that original graph.
pub fn lift_h1_classes(
    collapsed: &CollapsedRips,
    mut explained: ExplainedDiagram,
) -> Result<ExplainedDiagram> {
    let original = reconstruct_input(collapsed)?;
    verify_sparse(
        &original,
        collapsed.certificate.requested_threshold(),
        collapsed,
    )
    .map_err(|error| Error::InvalidInput(format!("collapse lift: {error}")))?;
    for space in &mut explained.spaces {
        for class in &mut space.basis {
            lift_one(collapsed, class)?;
            validate_h1_cocycle(&original, &class.cocycle)?;
        }
        recanonicalize_space(&original, space)?;
    }
    explained.spaces.sort_by(|a, b| {
        a.interval
            .birth
            .total_cmp(&b.interval.birth)
            .then(a.interval.death.total_cmp(&b.interval.death))
            .then(a.id.cmp(&b.id))
    });
    Ok(explained)
}

fn reconstruct_input(collapsed: &CollapsedRips) -> Result<SparseDistanceMatrix> {
    let mut triplets: Vec<_> = collapsed.matrix.edges().collect();
    triplets.extend(collapsed.certificate.steps().iter().map(|step| {
        let (u, v) = step.edge();
        (u, v, step.value())
    }));
    SparseDistanceMatrix::from_triplets(collapsed.certificate.vertex_count(), &triplets)
}

fn lift_one(collapsed: &CollapsedRips, class: &mut PersistentClass) -> Result<()> {
    let modulus = class.cocycle.modulus as u64;
    let scale = class.cocycle.scale;
    let mut live: FxHashMap<(usize, usize), f64> = collapsed
        .matrix
        .edges()
        .map(|(u, v, value)| ((u, v), value))
        .collect();
    let mut coefficients: FxHashMap<(usize, usize), u64> = class
        .cocycle
        .terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient as u64))
        .collect();

    for step in collapsed.certificate.steps().iter().rev() {
        let (u, v) = step.edge();
        if step.value() <= scale {
            let witness = step
                .witnesses()
                .iter()
                .rev()
                .find(|&&(start, _)| start <= scale)
                .map(|&(_, witness)| witness)
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "collapse lift: edge ({u}, {v}) has no witness at scale {scale}"
                    ))
                })?;
            for (a, b) in [(u, witness), (v, witness)] {
                let edge = canonical_edge(a, b);
                let Some(&value) = live.get(&edge) else {
                    return Err(Error::InvalidInput(format!(
                        "collapse lift: witness edge ({}, {}) is not live",
                        edge.0, edge.1
                    )));
                };
                if value > scale {
                    return Err(Error::InvalidInput(format!(
                        "collapse lift: witness edge ({}, {}) is born after scale {scale}",
                        edge.0, edge.1
                    )));
                }
            }
            let vw = oriented_coefficient(&coefficients, v, witness, modulus);
            let wu = oriented_coefficient(&coefficients, witness, u, modulus);
            let coefficient = (modulus - (vw + wu) % modulus) % modulus;
            if coefficient != 0 {
                coefficients.insert((u, v), coefficient);
            }
        }
        if live.insert((u, v), step.value()).is_some() {
            return Err(Error::InvalidInput(format!(
                "collapse lift: edge ({u}, {v}) is restored twice"
            )));
        }
    }
    class.cocycle.terms = normalize_map(coefficients, modulus)?;
    Ok(())
}

fn canonical_edge(u: usize, v: usize) -> (usize, usize) {
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
            .map(|(basis_index, cocycle)| PersistentClass {
                id: basis_class_id(id, basis_index, &cocycle),
                group_id: id,
                basis_index,
                interval,
                cocycle,
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

fn normalize_map(
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
    let mut adjacency = vec![Vec::new(); vertex_count];
    for &(u, v) in active_edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    for row in &mut adjacency {
        row.sort_unstable();
    }
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
                        Some((base + oriented_coefficient(&coefficients, u, v, modulus)) % modulus);
                    queue.push_back(v);
                }
            }
        }
    }
    let mut row = BTreeMap::new();
    for &(u, v) in active_edges {
        let original = coefficient(&coefficients, u, v);
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

fn recanonicalize_space(
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
        .map(|(basis_index, cocycle)| PersistentClass {
            id: basis_class_id(id, basis_index, &cocycle),
            group_id: id,
            basis_index,
            interval: space.interval,
            cocycle,
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
    IntervalGroupId(hash.finalize().into())
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
    BasisClassId(hash.finalize().into())
}

/// Check that an H1 cocycle is canonical, closed, and nontrivial at its
/// declared scale.
pub fn validate_h1_cocycle(matrix: &SparseDistanceMatrix, cocycle: &Cocycle) -> Result<()> {
    let modulus = cocycle.modulus as u64;
    if !is_prime(modulus) || modulus >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )));
    }
    if cocycle.scale.is_nan() || cocycle.scale < 0.0 || !cocycle.scale.is_finite() {
        return Err(Error::InvalidInput(format!(
            "cocycle scale must be finite and non-negative, got {}",
            cocycle.scale
        )));
    }
    if cocycle.terms.is_empty() {
        return Err(Error::InvalidInput("cocycle has no terms".into()));
    }
    if cocycle.terms[0].coefficient != 1 {
        return Err(Error::InvalidInput(
            "cocycle first coefficient must be one".into(),
        ));
    }
    let mut coefficients: FxHashMap<(usize, usize), u64> = FxHashMap::default();
    let mut previous = None;
    for (index, term) in cocycle.terms.iter().enumerate() {
        if term.u >= term.v || term.v >= matrix.len() {
            return Err(Error::InvalidInput(format!(
                "cocycle term {index} has invalid edge ({}, {})",
                term.u, term.v
            )));
        }
        if term.coefficient == 0 || term.coefficient >= cocycle.modulus {
            return Err(Error::InvalidInput(format!(
                "cocycle term {index} has coefficient {} outside 1..{}",
                term.coefficient, cocycle.modulus
            )));
        }
        if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
            return Err(Error::InvalidInput(
                "cocycle terms are not in strict endpoint order".into(),
            ));
        }
        let distance = matrix.get(term.u, term.v);
        if !distance.is_finite() || distance > cocycle.scale {
            return Err(Error::InvalidInput(format!(
                "cocycle term ({}, {}) is absent at scale {}",
                term.u, term.v, cocycle.scale
            )));
        }
        coefficients.insert((term.u, term.v), term.coefficient as u64);
        previous = Some((term.u, term.v));
    }

    let mut adjacency = vec![Vec::new(); matrix.len()];
    for (u, v, _) in matrix
        .edges()
        .filter(|&(_, _, value)| value <= cocycle.scale)
    {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    for u in 0..matrix.len() {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            let mut a = adjacency[u].partition_point(|&w| w <= v);
            let mut b = adjacency[v].partition_point(|&w| w <= v);
            while a < adjacency[u].len() && b < adjacency[v].len() {
                match adjacency[u][a].cmp(&adjacency[v][b]) {
                    std::cmp::Ordering::Less => a += 1,
                    std::cmp::Ordering::Greater => b += 1,
                    std::cmp::Ordering::Equal => {
                        let w = adjacency[u][a];
                        let uv = coefficient(&coefficients, u, v);
                        let uw = coefficient(&coefficients, u, w);
                        let vw = coefficient(&coefficients, v, w);
                        if (uv + vw + modulus - uw) % modulus != 0 {
                            return Err(Error::InvalidInput(format!(
                                "cocycle is not closed on triangle ({u}, {v}, {w})"
                            )));
                        }
                        a += 1;
                        b += 1;
                    }
                }
            }
        }
    }

    if is_vertex_coboundary(&adjacency, &coefficients, modulus) {
        return Err(Error::InvalidInput(
            "cocycle is a vertex coboundary at its scale".into(),
        ));
    }
    Ok(())
}

fn coefficient(coefficients: &FxHashMap<(usize, usize), u64>, u: usize, v: usize) -> u64 {
    *coefficients.get(&(u, v)).unwrap_or(&0)
}

fn oriented_coefficient(
    coefficients: &FxHashMap<(usize, usize), u64>,
    from: usize,
    to: usize,
    modulus: u64,
) -> u64 {
    if from < to {
        coefficient(coefficients, from, to)
    } else {
        let value = coefficient(coefficients, to, from);
        if value == 0 { 0 } else { modulus - value }
    }
}

fn is_vertex_coboundary(
    adjacency: &[Vec<usize>],
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
) -> bool {
    let mut potential = vec![None; adjacency.len()];
    let mut queue = VecDeque::new();
    for root in 0..adjacency.len() {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0u64);
        queue.push_back(root);
        while let Some(u) = queue.pop_front() {
            let base = potential[u].expect("queued vertex has a potential");
            for &v in &adjacency[u] {
                let expected = (base + oriented_coefficient(coefficients, u, v, modulus)) % modulus;
                match potential[v] {
                    None => {
                        potential[v] = Some(expected);
                        queue.push_back(v);
                    }
                    Some(value) if value != expected => return false,
                    Some(_) => {}
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn square_class_is_closed_nontrivial_and_stable() {
        let matrix = square();
        let mut first = None;
        for modulus in [2, 3, 5] {
            for threads in [1, 3] {
                let params = RipsParams::new(1)
                    .with_modulus(modulus)
                    .with_threads(threads);
                let explained = rips_persistence_with_classes_sparse(&matrix, &params).unwrap();
                assert_eq!(explained.class_count(), 1);
                let class = explained.classes().next().unwrap();
                assert_eq!(class.interval.birth, 1.0);
                assert_eq!(class.interval.death, 2.0);
                validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
                if modulus == 2 {
                    match first {
                        None => first = Some(class.clone()),
                        Some(ref expected) => assert_eq!(class, expected),
                    }
                }
            }
        }
    }

    #[test]
    fn validator_rejects_a_triangle_defect_and_a_coboundary() {
        let triangle =
            SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0)])
                .unwrap();
        let defect = Cocycle {
            modulus: 2,
            scale: 1.0,
            terms: vec![CocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            }],
        };
        assert!(
            validate_h1_cocycle(&triangle, &defect)
                .unwrap_err()
                .to_string()
                .contains("not closed")
        );

        let path = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 2, 1.0)]).unwrap();
        let coboundary = Cocycle {
            modulus: 2,
            scale: 1.0,
            terms: vec![CocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            }],
        };
        assert!(
            validate_h1_cocycle(&path, &coboundary)
                .unwrap_err()
                .to_string()
                .contains("vertex coboundary")
        );
    }

    #[test]
    fn random_graphs_return_one_valid_class_per_h1_interval() {
        let mut state = 0x17e2_a90c_4b65_d381u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..80 {
            let n = 5 + next() as usize % 9;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 2 {
                        triplets.push((u, v, (next() % 4) as f64));
                    }
                }
            }
            let matrix = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            for modulus in [2, 3, 5] {
                let params = RipsParams::new(1).with_modulus(modulus).with_threshold(3.0);
                let explained = rips_persistence_with_classes_sparse(&matrix, &params)
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
                assert_eq!(
                    explained.diagram.in_dim(1).count(),
                    explained.class_count(),
                    "case {case}, modulus {modulus}"
                );
                for class in explained.classes() {
                    validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
                }
            }
        }
    }

    #[test]
    fn collapse_lifts_and_atlases_verify_on_random_graphs() {
        let mut state = 0x8c4f_172d_b365_e902u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..64 {
            let n = 5 + next() as usize % 5;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 3 {
                        triplets.push((u, v, (1 + next() % 4) as f64));
                    }
                }
            }
            let matrix = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            for modulus in [2, 3] {
                let baseline = rips_persistence_with_classes_sparse(
                    &matrix,
                    &RipsParams::new(1).with_modulus(modulus),
                )
                .unwrap();
                let collapsed = crate::collapse::collapse_sparse(&matrix, None).unwrap();
                let mut reduced_params = RipsParams::new(1).with_modulus(modulus);
                reduced_params.threshold = Some(collapsed.certificate.terminal_level());
                let reduced =
                    rips_persistence_with_classes_sparse(&collapsed.matrix, &reduced_params)
                        .unwrap();
                let explained = lift_h1_classes(&collapsed, reduced)
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
                assert_eq!(explained.diagram.bars, baseline.diagram.bars);
                for class in explained.classes() {
                    validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
                }
                let params = RipsParams::new(1)
                    .with_modulus(modulus)
                    .with_edge_collapse();
                let artifact = crate::AtlasArtifact::build(
                    &matrix,
                    &params,
                    crate::CertificateLimits::default(),
                )
                .unwrap();
                artifact
                    .verify(&matrix, crate::CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            }
        }
    }

    #[test]
    fn duplicate_intervals_form_one_class_space_with_a_canonical_basis() {
        let matrix = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (5, 6, 1.0),
                (3, 6, 1.0),
                (3, 5, 2.0),
                (4, 6, 2.0),
            ],
        )
        .unwrap();
        let serial = rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1)).unwrap();
        let parallel =
            rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1).with_threads(4))
                .unwrap();
        assert_eq!(serial.spaces, parallel.spaces);
        assert_eq!(serial.spaces.len(), 1);
        assert_eq!(serial.spaces[0].basis.len(), 2);
        assert_eq!(serial.spaces[0].basis[0].group_id, serial.spaces[0].id);
        assert_eq!(serial.spaces[0].basis[1].group_id, serial.spaces[0].id);
        assert_ne!(serial.spaces[0].basis[0].id, serial.spaces[0].basis[1].id);
    }

    #[test]
    fn class_space_basis_is_canonical_for_any_seed_order() {
        let matrix = SparseDistanceMatrix::from_triplets(
            8,
            &[
                (0, 1, 1.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (4, 5, 1.0),
                (4, 7, 1.0),
                (5, 6, 1.0),
                (6, 7, 1.0),
            ],
        )
        .unwrap();
        let cocycle = |terms| Cocycle {
            modulus: 2,
            scale: 1.0,
            terms,
        };
        let first = CocycleTerm {
            u: 2,
            v: 3,
            coefficient: 1,
        };
        let second = CocycleTerm {
            u: 6,
            v: 7,
            coefficient: 1,
        };
        let seeds = [cocycle(vec![second]), cocycle(vec![first, second])];
        let expected = vec![cocycle(vec![first]), cocycle(vec![second])];
        let basis = canonical_space_basis(&matrix, 2, &seeds).unwrap();
        assert_eq!(basis, expected);
        assert_eq!(canonical_space_basis(&matrix, 2, &basis).unwrap(), basis);
    }

    #[test]
    fn every_collapse_schedule_lifts_classes_to_the_original_graph() {
        let matrix = square();
        for schedule in [
            crate::CollapseSchedule::Serial,
            crate::CollapseSchedule::Ordered,
            crate::CollapseSchedule::Rounds,
            crate::CollapseSchedule::Adaptive,
        ] {
            for modulus in [2, 3, 5] {
                let baseline = rips_persistence_with_classes_sparse(
                    &matrix,
                    &RipsParams::new(1).with_modulus(modulus),
                )
                .unwrap();
                let mut params = RipsParams::new(1)
                    .with_modulus(modulus)
                    .with_threads(3)
                    .with_collapse_schedule(schedule);
                if schedule == crate::CollapseSchedule::Adaptive {
                    params.adaptive_collapse = crate::collapse::AdaptiveCollapseParams::new(
                        crate::collapse::CollapseObjective::H1,
                    );
                }
                let explained = rips_persistence_with_classes_sparse(&matrix, &params)
                    .unwrap_or_else(|error| {
                        panic!("schedule {schedule:?}, modulus {modulus}: {error}")
                    });
                assert_eq!(explained.diagram.bars, baseline.diagram.bars);
                assert_eq!(explained.class_count(), 1);
                validate_h1_cocycle(&matrix, &explained.classes().next().unwrap().cocycle).unwrap();
                assert_eq!(
                    explained.spaces, baseline.spaces,
                    "schedule {schedule:?}, modulus {modulus}"
                );
            }
        }
    }
}
