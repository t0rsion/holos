//! Algebraic certificates for exact H0 and H1 persistence.
//!
//! The producer reduces explicit edge and triangle boundary matrices and
//! records their sparse change-of-basis columns. The checker does not call
//! the holos persistence solver. It reconstructs each original boundary,
//! checks the declared change of basis, requires distinct reduced pivots,
//! and derives the diagram from the checked columns.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{
    Bar, CriticalPair, CriticalSimplex, Diagram, Error, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse,
};

const MAGIC: &[u8; 8] = b"HOLOSRED";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing or checking an algebraic certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateError {
    message: String,
}

impl CertificateError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated certificate rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CertificateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "reduction certificate: {}", self.message)
    }
}

impl std::error::Error for CertificateError {}

/// Resource limits for certificate production and verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CertificateLimits {
    /// Largest accepted certificate envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted filtered edge count.
    pub max_edges: usize,
    /// Largest accepted filtered triangle count.
    pub max_triangles: usize,
    /// Largest accepted simplex count in any dimension above two.
    pub max_higher_simplices: usize,
    /// Largest homology dimension accepted by a graded certificate.
    pub max_dimension: usize,
    /// Largest accepted total change-of-basis term count.
    pub max_terms: usize,
    /// Largest accepted diagram bar count.
    pub max_bars: usize,
}

impl Default for CertificateLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_edges: 20_000_000,
            max_triangles: 100_000_000,
            max_higher_simplices: 100_000_000,
            max_dimension: 8,
            max_terms: 200_000_000,
            max_bars: 100_000_000,
        }
    }
}

/// One nonzero coefficient in a sparse change-of-basis column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CertificateTerm {
    /// Earlier or current source-column position.
    pub index: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One filtration-compatible change-of-basis column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeColumn {
    /// Nonzero terms in ascending source-column order.
    pub terms: Vec<CertificateTerm>,
}

/// How an existing checked reduction was adapted to a changed filtration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReductionRepairMode {
    /// Every reduction column remained valid in the same position.
    Reused,
    /// A stable prefix was reused and the remaining columns were reduced.
    SuffixRepaired,
    /// No reduction column could be reused.
    Rebuilt,
}

/// Exact algebraic work charged to one reduction repair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReductionRepairWork {
    /// Edge-boundary columns retained without reduction.
    pub edge_columns_reused: usize,
    /// Edge-boundary columns reduced after the retained prefix.
    pub edge_columns_reduced: usize,
    /// Triangle-boundary columns retained without reduction.
    pub triangle_columns_reused: usize,
    /// Triangle-boundary columns reduced after the retained prefix.
    pub triangle_columns_reduced: usize,
    /// Sparse column additions performed by the repair.
    pub column_additions: usize,
}

impl ReductionRepairWork {
    /// Total boundary columns in the repaired reduction.
    pub fn columns(&self) -> usize {
        self.edge_columns_reused
            + self.edge_columns_reduced
            + self.triangle_columns_reused
            + self.triangle_columns_reduced
    }

    /// Columns retained without another reduction pass.
    pub fn columns_reused(&self) -> usize {
        self.edge_columns_reused + self.triangle_columns_reused
    }

    /// Columns processed by the repair reduction.
    pub fn columns_reduced(&self) -> usize {
        self.edge_columns_reduced + self.triangle_columns_reduced
    }
}

/// A checked reduction adapted to a changed filtration.
#[derive(Debug, Clone)]
pub struct ReductionRepair {
    certificate: ReductionCertificate,
    mode: ReductionRepairMode,
    work: ReductionRepairWork,
}

impl ReductionRepair {
    /// Repaired certificate bound to the updated graph.
    pub fn certificate(&self) -> &ReductionCertificate {
        &self.certificate
    }

    /// How the reduction was adapted.
    pub fn mode(&self) -> ReductionRepairMode {
        self.mode
    }

    /// Exact reduction work charged by the operation.
    pub fn work(&self) -> ReductionRepairWork {
        self.work
    }

    pub(crate) fn into_certificate(self) -> ReductionCertificate {
        self.certificate
    }
}

/// One simplex named by its ascending vertex labels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FiltrationSimplex {
    vertices: Vec<usize>,
}

impl FiltrationSimplex {
    fn new(vertices: impl Into<Vec<usize>>) -> Self {
        Self {
            vertices: vertices.into(),
        }
    }

    /// Simplex dimension.
    pub fn dimension(&self) -> usize {
        self.vertices.len().saturating_sub(1)
    }

    /// Ascending vertex labels.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }
}

/// Algebraic reason that one filtration comparison must remain true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ReductionGuardKind {
    /// A source term in `V` must not follow its target column.
    ChangeOfBasis,
    /// A reduced-column term must not follow the declared pivot.
    Pivot,
}

/// One comparison sufficient to preserve a checked reduction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReductionGuard {
    kind: ReductionGuardKind,
    earlier: FiltrationSimplex,
    later: FiltrationSimplex,
}

impl ReductionGuard {
    /// Why the comparison is required.
    pub fn kind(&self) -> ReductionGuardKind {
        self.kind
    }

    /// Simplex that must not follow [`Self::later`].
    pub fn earlier(&self) -> &FiltrationSimplex {
        &self.earlier
    }

    /// Simplex that must not precede [`Self::earlier`].
    pub fn later(&self) -> &FiltrationSimplex {
        &self.later
    }
}

/// Kind of failed condition in a certified reduction region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegionViolationKind {
    /// The labeled vertex set changed.
    VertexSetChanged,
    /// The complete listed edge set changed.
    EdgeSetChanged,
    /// An edge crossed the fixed threshold.
    ThresholdCrossing,
    /// A required filtration comparison reversed.
    GuardFailed,
}

/// One condition that prevents reuse of a certified reduction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionViolation {
    kind: RegionViolationKind,
    guard_index: Option<usize>,
    first: Option<FiltrationSimplex>,
    second: Option<FiltrationSimplex>,
}

impl RegionViolation {
    /// Kind of failed condition.
    pub fn kind(&self) -> RegionViolationKind {
        self.kind
    }

    /// Index in [`CertifiedReductionRegion::guards`], when a guard failed.
    pub fn guard_index(&self) -> Option<usize> {
        self.guard_index
    }

    /// First affected simplex, when one is available.
    pub fn first(&self) -> Option<&FiltrationSimplex> {
        self.first.as_ref()
    }

    /// Second affected simplex, when one is available.
    pub fn second(&self) -> Option<&FiltrationSimplex> {
        self.second.as_ref()
    }
}

/// Exact result obtained by reusing one checked algebraic reduction.
#[derive(Debug, Clone)]
pub struct CertifiedRegionEvaluation {
    diagram: Diagram,
    h1_pairs: Vec<(Bar, CriticalPair)>,
    guards_checked: usize,
}

impl CertifiedRegionEvaluation {
    /// Exact H0 and H1 diagram at the updated weights.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Positive H1 intervals and their unchanged critical simplices.
    pub fn h1_critical_pairs(&self) -> &[(Bar, CriticalPair)] {
        &self.h1_pairs
    }

    /// Number of algebraic comparisons checked for this evaluation.
    pub fn guards_checked(&self) -> usize {
        self.guards_checked
    }
}

#[derive(Debug, Clone)]
struct RegionH1Pair {
    birth: [usize; 2],
    death: Option<[usize; 3]>,
}

#[derive(Debug, Clone, Copy)]
enum RegionValueFormula {
    Vertex,
    Edge(usize),
    Triangle([usize; 3]),
}

impl RegionValueFormula {
    fn value(self, edge_value: impl Fn(usize) -> f64) -> f64 {
        match self {
            Self::Vertex => 0.0,
            Self::Edge(edge) => edge_value(edge),
            Self::Triangle([first, second, third]) => edge_value(first)
                .max(edge_value(second))
                .max(edge_value(third)),
        }
    }
}

/// Reusable exact H0 and H1 reduction under result-sensitive guards.
///
/// The region fixes the labeled graph and threshold membership. It does not
/// fix the complete weak edge order. Reuse is valid while every declared
/// change-of-basis and pivot guard remains true.
#[derive(Debug, Clone)]
pub struct CertifiedReductionRegion {
    vertex_count: usize,
    threshold: Option<f64>,
    topology: Vec<[usize; 2]>,
    active: Vec<bool>,
    guards: Vec<ReductionGuard>,
    guard_indices: Vec<(usize, usize)>,
    guard_ranks: Vec<u128>,
    guard_formulas: Vec<RegionValueFormula>,
    h0_deaths: Vec<[usize; 2]>,
    h0_essential: usize,
    h1_pairs: Vec<RegionH1Pair>,
    h1_formulas: Vec<(usize, Option<[usize; 3]>)>,
}

impl CertifiedReductionRegion {
    /// Labeled vertex count fixed by the region.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Minimal checked comparisons derived from the certified reduction.
    pub fn guards(&self) -> &[ReductionGuard] {
        &self.guards
    }

    /// Return every failed topology or algebraic condition.
    pub fn violations(&self, updated: &SparseDistanceMatrix) -> Vec<RegionViolation> {
        let mut violations = Vec::new();
        if updated.len() != self.vertex_count {
            violations.push(RegionViolation {
                kind: RegionViolationKind::VertexSetChanged,
                guard_index: None,
                first: None,
                second: None,
            });
            return violations;
        }
        let topology: Vec<_> = updated.edges().map(|(u, v, _)| [u, v]).collect();
        if topology != self.topology {
            let first = self
                .topology
                .iter()
                .find(|edge| topology.binary_search(edge).is_err())
                .or_else(|| {
                    topology
                        .iter()
                        .find(|edge| self.topology.binary_search(edge).is_err())
                })
                .copied()
                .map(FiltrationSimplex::new);
            violations.push(RegionViolation {
                kind: RegionViolationKind::EdgeSetChanged,
                guard_index: None,
                first,
                second: None,
            });
            return violations;
        }
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        for (index, [u, v]) in self.topology.iter().copied().enumerate() {
            if (updated.get(u, v) <= threshold) != self.active[index] {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::ThresholdCrossing,
                    guard_index: None,
                    first: Some(FiltrationSimplex::new(vec![u, v])),
                    second: None,
                });
            }
        }
        if !violations.is_empty() {
            return violations;
        }
        let edge_values: Vec<_> = updated.edges().map(|(_, _, value)| value).collect();
        let values: Vec<_> = self
            .guard_formulas
            .iter()
            .map(|formula| formula.value(|edge| edge_values[edge]))
            .collect();
        for (index, (&(earlier, later), guard)) in
            self.guard_indices.iter().zip(&self.guards).enumerate()
        {
            let order = values[earlier]
                .total_cmp(&values[later])
                .then_with(|| self.guard_ranks[later].cmp(&self.guard_ranks[earlier]));
            if order.is_gt() {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::GuardFailed,
                    guard_index: Some(index),
                    first: Some(guard.earlier.clone()),
                    second: Some(guard.later.clone()),
                });
            }
        }
        violations
    }

    /// Evaluate the exact diagram without reduction when every guard holds.
    pub fn evaluate(
        &self,
        updated: &SparseDistanceMatrix,
    ) -> std::result::Result<CertifiedRegionEvaluation, CertificateError> {
        let violations = self.violations(updated);
        if let Some(first) = violations.first() {
            return Err(CertificateError::new(format!(
                "certified region ended at {:?}",
                first.kind
            )));
        }
        let mut diagram = Diagram::default();
        for &[u, v] in &self.h0_deaths {
            let death = updated.get(u, v);
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
        for _ in 0..self.h0_essential {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
        let mut h1_pairs = Vec::new();
        for pair in &self.h1_pairs {
            let birth = updated.get(pair.birth[0], pair.birth[1]);
            let death = pair
                .death
                .map(|vertices| triangle_value(updated, vertices))
                .unwrap_or(f64::INFINITY);
            if death > birth {
                let interval = Bar {
                    dim: 1,
                    birth,
                    death,
                };
                diagram.bars.push(interval);
                h1_pairs.push((
                    interval,
                    CriticalPair {
                        birth: CriticalSimplex {
                            vertices: pair.birth.to_vec(),
                            value: birth,
                        },
                        death: pair.death.map(|vertices| CriticalSimplex {
                            vertices: vertices.to_vec(),
                            value: triangle_value(updated, vertices),
                        }),
                    },
                ));
            }
        }
        diagram.canonicalize();
        h1_pairs.sort_by(critical_pair_record_order);
        Ok(CertifiedRegionEvaluation {
            diagram,
            h1_pairs,
            guards_checked: self.guards.len(),
        })
    }

    pub(crate) fn evaluate_h1_indexed(
        &self,
        edge_values: &[f64],
        edge_positions: &[usize],
    ) -> std::result::Result<Vec<Bar>, CertificateError> {
        debug_assert_eq!(edge_positions.len(), self.topology.len());
        let edge_value = |edge: usize| edge_values[edge_positions[edge]];
        let values: Vec<_> = self
            .guard_formulas
            .iter()
            .map(|formula| formula.value(edge_value))
            .collect();
        for &(earlier, later) in &self.guard_indices {
            let order = values[earlier]
                .total_cmp(&values[later])
                .then_with(|| self.guard_ranks[later].cmp(&self.guard_ranks[earlier]));
            if order.is_gt() {
                return Err(CertificateError::new(
                    "certified region ended at GuardFailed",
                ));
            }
        }
        let mut bars = Vec::new();
        for &(birth_edge, death_edges) in &self.h1_formulas {
            let birth = edge_value(birth_edge);
            let death = death_edges
                .map(|[first, second, third]| {
                    edge_value(first)
                        .max(edge_value(second))
                        .max(edge_value(third))
                })
                .unwrap_or(f64::INFINITY);
            if death > birth {
                bars.push(Bar {
                    dim: 1,
                    birth,
                    death,
                });
            }
        }
        bars.sort_by(|left, right| {
            left.birth
                .total_cmp(&right.birth)
                .then(left.death.total_cmp(&right.death))
        });
        Ok(bars)
    }
}

/// Proof that a filtered boundary matrix has the declared reduced pivots.
#[derive(Debug, Clone)]
pub struct ReductionCertificate {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    graph_digest: [u8; 32],
    edge_columns: Vec<ChangeColumn>,
    triangle_columns: Vec<ChangeColumn>,
    diagram: Diagram,
}

type CertificateResult<T> = std::result::Result<T, CertificateError>;

fn build_checked_reductions(
    input: &SparseDistanceMatrix,
    modulus: u32,
    threshold: f64,
    limits: CertificateLimits,
) -> CertificateResult<(Vec<ChangeColumn>, Vec<ChangeColumn>, Diagram)> {
    let complex = FilteredComplex::build(input, threshold, limits)?;
    let edge_columns = reduce_with_basis(&complex.edge_boundaries(modulus), modulus, limits)?;
    let triangle_columns =
        reduce_with_basis(&complex.triangle_boundaries(modulus), modulus, limits)?;
    let checked = check_reductions(&complex, modulus, &edge_columns, &triangle_columns, limits)?;
    Ok((edge_columns, triangle_columns, checked.diagram))
}

fn check_compute_diagram(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    checked: &Diagram,
) -> CertificateResult<()> {
    let computed = rips_persistence_sparse(input, params)
        .map_err(|error| CertificateError::new(error.to_string()))?;
    if !diagram_bits_equal(&computed, checked) {
        return Err(CertificateError::new(format!(
            "reference certificate diagram differs from the compute engine: expected {:?}, got {:?}",
            computed.bars, checked.bars
        )));
    }
    Ok(())
}

struct DimensionRepair {
    columns: Vec<ChangeColumn>,
    prefix: usize,
    additions: usize,
}

fn complex_edges(complex: &FilteredComplex) -> Vec<[usize; 2]> {
    complex
        .edges
        .iter()
        .map(|simplex| simplex.vertices)
        .collect()
}

fn complex_triangles(complex: &FilteredComplex) -> Vec<[usize; 3]> {
    complex
        .triangles
        .iter()
        .map(|simplex| simplex.vertices)
        .collect()
}

fn check_update_topology(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    operation: &str,
) -> CertificateResult<()> {
    if current.len() != updated.len() {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires an unchanged vertex set"
        )));
    }
    let current_topology: Vec<_> = current.edges().map(|(u, v, _)| [u, v]).collect();
    let updated_topology: Vec<_> = updated.edges().map(|(u, v, _)| [u, v]).collect();
    if current_topology != updated_topology {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires an unchanged listed edge set"
        )));
    }
    Ok(())
}

fn check_update_contract(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    threshold: f64,
    operation: &str,
) -> CertificateResult<()> {
    check_update_topology(current, updated, operation)?;
    let current_active: Vec<_> = current
        .edges()
        .map(|(_, _, value)| value <= threshold)
        .collect();
    let updated_active: Vec<_> = updated
        .edges()
        .map(|(_, _, value)| value <= threshold)
        .collect();
    if current_active != updated_active {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires unchanged threshold membership"
        )));
    }
    Ok(())
}

fn repair_dimension<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> CertificateResult<DimensionRepair> {
    let candidates = reindexed_prefix_candidates(old_simplices, new_simplices, old_columns)?;
    let prefix = valid_reduction_prefix_len(boundaries, &candidates, modulus, limits)?;
    let (columns, additions) =
        reduce_with_prefix(boundaries, &candidates[..prefix], modulus, limits)?;
    Ok(DimensionRepair {
        columns,
        prefix,
        additions,
    })
}

fn repair_mode(work: ReductionRepairWork) -> ReductionRepairMode {
    if work.columns_reduced() == 0 {
        ReductionRepairMode::Reused
    } else if work.columns_reused() == 0 {
        ReductionRepairMode::Rebuilt
    } else {
        ReductionRepairMode::SuffixRepaired
    }
}

#[derive(Clone, Copy)]
struct RecordCounts {
    edges: usize,
    triangles: usize,
    bars: usize,
}

struct DecodedHeader {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    graph_digest: [u8; 32],
    counts: RecordCounts,
}

fn check_envelope_size(bytes: &[u8], limits: CertificateLimits) -> CertificateResult<()> {
    if bytes.len() > limits.max_bytes {
        return Err(CertificateError::new(format!(
            "{} bytes exceed the limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    Ok(())
}

fn check_wire_preamble(reader: &mut Reader<'_>) -> CertificateResult<()> {
    if reader.take(8)? != MAGIC {
        return Err(CertificateError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(CertificateError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(CertificateError::new(format!(
            "unsupported scalar codec {codec}"
        )));
    }
    Ok(())
}

fn decode_record_counts(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> CertificateResult<RecordCounts> {
    Ok(RecordCounts {
        edges: reader.bounded_usize("edge column count", limits.max_edges)?,
        triangles: reader.bounded_usize("triangle column count", limits.max_triangles)?,
        bars: reader.bounded_usize("bar count", limits.max_bars)?,
    })
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> CertificateResult<DecodedHeader> {
    check_wire_preamble(reader)?;
    let modulus = reader.u32()?;
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let threshold = reader.optional_f64()?;
    let counts = decode_record_counts(reader, limits)?;
    let graph_digest = reader.array32()?;
    Ok(DecodedHeader {
        vertex_count,
        threshold,
        modulus,
        graph_digest,
        counts,
    })
}

fn check_minimum_record_bytes(reader: &Reader<'_>, counts: RecordCounts) -> CertificateResult<()> {
    let minimum = counts
        .edges
        .checked_add(counts.triangles)
        .and_then(|count| count.checked_mul(20))
        .and_then(|bytes| {
            counts
                .bars
                .checked_mul(24)
                .and_then(|bars| bytes.checked_add(bars))
        })
        .ok_or_else(|| CertificateError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(CertificateError::new(format!(
            "record counts need at least {minimum} bytes, only {} remain",
            reader.remaining()
        )));
    }
    Ok(())
}

fn decode_bars(reader: &mut Reader<'_>, count: usize) -> CertificateResult<Vec<Bar>> {
    let mut bars = Vec::with_capacity(count);
    for _ in 0..count {
        bars.push(Bar {
            dim: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    Ok(bars)
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> CertificateResult<()> {
    if reader.remaining() != 0 {
        return Err(CertificateError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}

impl ReductionCertificate {
    /// Produce an exact H0 and H1 reduction certificate.
    ///
    /// The producer uses an explicit reference reduction. It is separate
    /// from the implicit compute engine and is limited by
    /// [`CertificateLimits`].
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        validate_header(input, params.max_dim, params.modulus, limits)?;
        let threshold = checked_threshold(params.threshold)?;
        let (edge_columns, triangle_columns, diagram) =
            build_checked_reductions(input, params.modulus, threshold, limits)?;
        check_compute_diagram(input, params, &diagram)?;
        Ok(Self {
            vertex_count: input.len(),
            threshold: params.threshold,
            modulus: params.modulus,
            graph_digest: graph_digest(input, threshold),
            edge_columns,
            triangle_columns,
            diagram,
        })
    }

    /// Adapt this checked reduction to new weights on the same listed graph.
    ///
    /// The repair reindexes dependencies by simplex identity in each boundary
    /// dimension. It retains the longest unit-triangular prefix whose
    /// recomputed columns still have distinct pivots, then reduces the suffix.
    ///
    /// The current graph must match this certificate. The updated graph must
    /// keep the vertex set, listed edges, and threshold membership fixed.
    pub fn repair(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<ReductionRepair, CertificateError> {
        let (old_complex, _) = self.verify_parts(current, limits)?;
        let threshold = checked_threshold(self.threshold)?;
        check_update_contract(current, updated, threshold, "repair")?;
        let new_complex = FilteredComplex::build(updated, threshold, limits)?;
        let edge_repair = repair_dimension(
            &complex_edges(&old_complex),
            &complex_edges(&new_complex),
            &self.edge_columns,
            &new_complex.edge_boundaries(self.modulus),
            self.modulus,
            limits,
        )?;
        let triangle_repair = repair_dimension(
            &complex_triangles(&old_complex),
            &complex_triangles(&new_complex),
            &self.triangle_columns,
            &new_complex.triangle_boundaries(self.modulus),
            self.modulus,
            limits,
        )?;
        let checked = check_reductions(
            &new_complex,
            self.modulus,
            &edge_repair.columns,
            &triangle_repair.columns,
            limits,
        )?;
        let certificate = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            graph_digest: graph_digest(updated, threshold),
            edge_columns: edge_repair.columns,
            triangle_columns: triangle_repair.columns,
            diagram: checked.diagram,
        };
        let work = ReductionRepairWork {
            edge_columns_reused: edge_repair.prefix,
            edge_columns_reduced: new_complex.edges.len() - edge_repair.prefix,
            triangle_columns_reused: triangle_repair.prefix,
            triangle_columns_reduced: new_complex.triangles.len() - triangle_repair.prefix,
            column_additions: edge_repair.additions + triangle_repair.additions,
        };
        Ok(ReductionRepair {
            certificate,
            mode: repair_mode(work),
            work,
        })
    }

    /// Reindex this reduction after a result-sensitive accepted update.
    ///
    /// No boundary column is reduced. Simplex identities remap every source
    /// and target position in `V`.
    pub fn reindex(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        let (old_complex, _) = self.verify_parts(current, limits)?;
        let threshold = checked_threshold(self.threshold)?;
        check_update_topology(current, updated, "reindexing")?;
        let new_complex = FilteredComplex::build(updated, threshold, limits)?;
        if old_complex.edges.len() != new_complex.edges.len()
            || old_complex.triangles.len() != new_complex.triangles.len()
        {
            return Err(CertificateError::new(
                "reduction reindexing requires unchanged threshold membership",
            ));
        }
        let edge_columns = reindex_change_columns(
            &complex_edges(&old_complex),
            &complex_edges(&new_complex),
            &self.edge_columns,
        )?;
        let triangle_columns = reindex_change_columns(
            &complex_triangles(&old_complex),
            &complex_triangles(&new_complex),
            &self.triangle_columns,
        )?;
        let checked = check_reductions(
            &new_complex,
            self.modulus,
            &edge_columns,
            &triangle_columns,
            limits,
        )?;
        Ok(Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            graph_digest: graph_digest(updated, threshold),
            edge_columns,
            triangle_columns,
            diagram: checked.diagram,
        })
    }

    /// Vertex count bound to this certificate.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Filtration threshold bound to this certificate.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Digest of the thresholded graph.
    pub fn graph_digest(&self) -> &[u8; 32] {
        &self.graph_digest
    }

    /// Edge-boundary change-of-basis columns.
    pub fn edge_columns(&self) -> &[ChangeColumn] {
        &self.edge_columns
    }

    /// Triangle-boundary change-of-basis columns.
    pub fn triangle_columns(&self) -> &[ChangeColumn] {
        &self.triangle_columns
    }

    /// Diagram derived from the checked reduction.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Encode the canonical `HOLOSRED` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, CertificateError> {
        self.check_envelope(CertificateLimits::default())?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, WIRE_VERSION);
        out.push(F64_BITS_CODEC);
        put_u32(&mut out, self.modulus);
        put_usize(&mut out, self.vertex_count, "vertex count")?;
        put_optional_f64(&mut out, self.threshold);
        put_usize(&mut out, self.edge_columns.len(), "edge column count")?;
        put_usize(
            &mut out,
            self.triangle_columns.len(),
            "triangle column count",
        )?;
        put_usize(&mut out, self.diagram.bars.len(), "bar count")?;
        out.extend_from_slice(&self.graph_digest);
        encode_columns(&mut out, &self.edge_columns)?;
        encode_columns(&mut out, &self.triangle_columns)?;
        for bar in &self.diagram.bars {
            put_usize(&mut out, bar.dim, "bar dimension")?;
            put_u64(&mut out, bar.birth.to_bits());
            put_u64(&mut out, bar.death.to_bits());
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded `HOLOSRED` envelope.
    pub fn decode(
        bytes: &[u8],
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        check_envelope_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        let header = decode_header(&mut reader, limits)?;
        check_minimum_record_bytes(&reader, header.counts)?;
        let mut total_terms = 0usize;
        let edge_columns = decode_columns(
            &mut reader,
            header.counts.edges,
            header.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        let triangle_columns = decode_columns(
            &mut reader,
            header.counts.triangles,
            header.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        let bars = decode_bars(&mut reader, header.counts.bars)?;
        check_no_trailing_bytes(&reader)?;
        let certificate = Self {
            vertex_count: header.vertex_count,
            threshold: header.threshold,
            modulus: header.modulus,
            graph_digest: header.graph_digest,
            edge_columns,
            triangle_columns,
            diagram: Diagram { bars },
        };
        certificate.check_envelope(limits)?;
        Ok(certificate)
    }

    /// Verify the change of basis, distinct pivots, and derived diagram.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<Diagram, CertificateError> {
        Ok(self.verify_checked(input, limits)?.diagram)
    }

    /// Compile the checked factorization into a result-sensitive region.
    ///
    /// The region stays valid under edge-order changes that do not break
    /// a filtration dependency in `V` or a reduced pivot in `R`.
    pub fn compile_region(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<CertifiedReductionRegion, CertificateError> {
        let (complex, checked) = self.verify_parts(input, limits)?;
        let edge_simplices: Vec<_> = complex
            .edges
            .iter()
            .map(|edge| FiltrationSimplex::new(edge.vertices.to_vec()))
            .collect();
        let triangle_simplices: Vec<_> = complex
            .triangles
            .iter()
            .map(|triangle| FiltrationSimplex::new(triangle.vertices.to_vec()))
            .collect();
        let mut guards = BTreeSet::new();
        add_change_guards(&mut guards, &self.edge_columns, &edge_simplices);
        add_change_guards(&mut guards, &self.triangle_columns, &triangle_simplices);
        add_pivot_guards(&mut guards, &checked.reduced_triangles, &edge_simplices);

        let mut killed_vertices = vec![false; complex.vertex_count];
        let mut h0_deaths = Vec::new();
        for (column, reduced) in checked.reduced_edges.iter().enumerate() {
            if let Some((pivot, _)) = reduced.pivot() {
                killed_vertices[pivot] = true;
                h0_deaths.push(complex.edges[column].vertices);
            }
        }
        let h0_essential = killed_vertices.iter().filter(|&&killed| !killed).count();
        let mut h1_deaths = FxHashMap::default();
        for (column, reduced) in checked.reduced_triangles.iter().enumerate() {
            if let Some((pivot, _)) = reduced.pivot() {
                h1_deaths.insert(pivot, complex.triangles[column].vertices);
            }
        }
        let h1_pairs: Vec<_> = checked
            .reduced_edges
            .iter()
            .enumerate()
            .filter(|(_, reduced)| reduced.0.is_empty())
            .map(|(edge, _)| RegionH1Pair {
                birth: complex.edges[edge].vertices,
                death: h1_deaths.get(&edge).copied(),
            })
            .collect();
        let threshold = checked_threshold(self.threshold)?;
        let topology: Vec<_> = input.edges().map(|(u, v, _)| [u, v]).collect();
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let guards = minimize_guards(guards);
        let mut simplex_indices = BTreeMap::new();
        for guard in &guards {
            for simplex in [&guard.earlier, &guard.later] {
                let next = simplex_indices.len();
                simplex_indices.entry(simplex.clone()).or_insert(next);
            }
        }
        let mut guard_simplices = vec![FiltrationSimplex::new(Vec::new()); simplex_indices.len()];
        for (simplex, index) in &simplex_indices {
            guard_simplices[*index] = simplex.clone();
        }
        let guard_indices = guards
            .iter()
            .map(|guard| {
                (
                    simplex_indices[&guard.earlier],
                    simplex_indices[&guard.later],
                )
            })
            .collect();
        let guard_ranks = guard_simplices.iter().map(simplex_rank).collect();
        let edge_indices: BTreeMap<_, _> = topology
            .iter()
            .copied()
            .enumerate()
            .map(|(index, edge)| (edge, index))
            .collect();
        let guard_formulas = guard_simplices
            .iter()
            .map(|simplex| region_value_formula(simplex, &edge_indices))
            .collect();
        let h1_formulas = h1_pairs
            .iter()
            .map(|pair| {
                (
                    edge_indices[&pair.birth],
                    pair.death.map(|[u, v, w]| {
                        [
                            edge_indices[&[u, v]],
                            edge_indices[&[u, w]],
                            edge_indices[&[v, w]],
                        ]
                    }),
                )
            })
            .collect();
        Ok(CertifiedReductionRegion {
            vertex_count: input.len(),
            threshold: self.threshold,
            topology,
            active,
            guards,
            guard_indices,
            guard_ranks,
            guard_formulas,
            h0_deaths,
            h0_essential,
            h1_pairs,
            h1_formulas,
        })
    }

    fn verify_checked(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<CheckedReductions, CertificateError> {
        Ok(self.verify_parts(input, limits)?.1)
    }

    fn verify_parts(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<(FilteredComplex, CheckedReductions), CertificateError> {
        let params = RipsParams::new(1).with_modulus(self.modulus);
        validate_header(input, params.max_dim, params.modulus, limits)?;
        if input.len() != self.vertex_count {
            return Err(CertificateError::new(format!(
                "input has {} vertices, certificate records {}",
                input.len(),
                self.vertex_count
            )));
        }
        let threshold = checked_threshold(self.threshold)?;
        if graph_digest(input, threshold) != self.graph_digest {
            return Err(CertificateError::new(
                "thresholded graph binding does not match",
            ));
        }
        let complex = FilteredComplex::build(input, threshold, limits)?;
        let checked = check_reductions(
            &complex,
            self.modulus,
            &self.edge_columns,
            &self.triangle_columns,
            limits,
        )?;
        if !diagram_bits_equal(&checked.diagram, &self.diagram) {
            return Err(CertificateError::new(
                "recorded diagram differs from the checked reduction",
            ));
        }
        Ok((complex, checked))
    }

    pub(crate) fn verify_with_h1_critical_pairs(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<(Diagram, Vec<(Bar, CriticalPair)>), CertificateError> {
        let checked = self.verify_checked(input, limits)?;
        Ok((checked.diagram, checked.h1_pairs))
    }

    fn check_envelope(
        &self,
        limits: CertificateLimits,
    ) -> std::result::Result<(), CertificateError> {
        self.check_envelope_header(limits)?;
        let mut total_terms = 0usize;
        check_change_columns(
            "edge",
            &self.edge_columns,
            self.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        check_change_columns(
            "triangle",
            &self.triangle_columns,
            self.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        check_bars(&self.diagram, limits.max_bars)
    }

    fn check_envelope_header(&self, limits: CertificateLimits) -> CertificateResult<()> {
        if self.vertex_count > limits.max_vertices {
            return Err(CertificateError::new(format!(
                "{} vertices exceed the limit {}",
                self.vertex_count, limits.max_vertices
            )));
        }
        if !is_prime(self.modulus as u64) || self.modulus as u64 >= MODULUS_LIMIT {
            return Err(CertificateError::new(format!(
                "modulus must be a prime below {MODULUS_LIMIT}, got {}",
                self.modulus
            )));
        }
        checked_threshold(self.threshold)?;
        if self.edge_columns.len() > limits.max_edges {
            return Err(CertificateError::new(format!(
                "{} edge columns exceed the limit {}",
                self.edge_columns.len(),
                limits.max_edges
            )));
        }
        if self.triangle_columns.len() > limits.max_triangles {
            return Err(CertificateError::new(format!(
                "{} triangle columns exceed the limit {}",
                self.triangle_columns.len(),
                limits.max_triangles
            )));
        }
        Ok(())
    }
}

fn check_bars(diagram: &Diagram, max_bars: usize) -> CertificateResult<()> {
    if diagram.bars.len() > max_bars {
        return Err(CertificateError::new(format!(
            "{} bars exceed the limit {max_bars}",
            diagram.bars.len()
        )));
    }
    for (index, bar) in diagram.bars.iter().enumerate() {
        if !canonical_bar(bar) {
            return Err(CertificateError::new(format!(
                "bar {index} is not canonical"
            )));
        }
    }
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&canonical, diagram) {
        return Err(CertificateError::new("bars are not in canonical order"));
    }
    Ok(())
}

fn canonical_bar(bar: &Bar) -> bool {
    bar.dim <= 1 && canonical_birth(bar.birth) && canonical_death(bar.birth, bar.death)
}

fn canonical_birth(value: f64) -> bool {
    !value.is_nan() && value.is_finite() && value >= 0.0 && (value != 0.0 || value.to_bits() == 0)
}

fn canonical_death(birth: f64, death: f64) -> bool {
    !death.is_nan() && death >= 0.0 && death > birth && (death != 0.0 || death.to_bits() == 0)
}

fn check_change_columns(
    label: &str,
    columns: &[ChangeColumn],
    modulus: u32,
    max_terms: usize,
    total_terms: &mut usize,
) -> std::result::Result<(), CertificateError> {
    for (column_index, column) in columns.iter().enumerate() {
        add_term_count(total_terms, column.terms.len(), max_terms)?;
        check_change_column(label, column_index, column, modulus)?;
    }
    Ok(())
}

fn add_term_count(total: &mut usize, count: usize, maximum: usize) -> CertificateResult<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("certificate term count overflows usize"))?;
    if *total > maximum {
        return Err(CertificateError::new(format!(
            "{} terms exceed the limit {maximum}",
            *total
        )));
    }
    Ok(())
}

fn check_change_column(
    label: &str,
    column_index: usize,
    column: &ChangeColumn,
    modulus: u32,
) -> CertificateResult<()> {
    if column.terms.is_empty() {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} is empty"
        )));
    }
    let mut previous = None;
    for (term_index, term) in column.terms.iter().enumerate() {
        check_change_term(label, column_index, term_index, term, previous, modulus)?;
        previous = Some(term.index);
    }
    let unit = CertificateTerm {
        index: column_index,
        coefficient: 1,
    };
    if column.terms.last() != Some(&unit) {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} is not unit triangular"
        )));
    }
    Ok(())
}

fn check_change_term(
    label: &str,
    column_index: usize,
    term_index: usize,
    term: &CertificateTerm,
    previous: Option<usize>,
    modulus: u32,
) -> CertificateResult<()> {
    if term.index > column_index {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} term {term_index} points forward"
        )));
    }
    if previous.is_some_and(|previous| previous >= term.index) {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} terms are not strictly ordered"
        )));
    }
    if term.coefficient == 0 || term.coefficient >= modulus {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} has invalid coefficient {}",
            term.coefficient
        )));
    }
    Ok(())
}

fn encode_columns(
    out: &mut Vec<u8>,
    columns: &[ChangeColumn],
) -> std::result::Result<(), CertificateError> {
    for column in columns {
        put_usize(out, column.terms.len(), "change-column term count")?;
        for term in &column.terms {
            put_usize(out, term.index, "change-column source index")?;
            put_u32(out, term.coefficient);
        }
    }
    Ok(())
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    max_terms: usize,
    total_terms: &mut usize,
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    let mut columns = Vec::with_capacity(count);
    for column_index in 0..count {
        columns.push(decode_column(reader, column_index, max_terms, total_terms)?);
    }
    let mut checked = 0;
    check_change_columns("decoded", &columns, modulus, max_terms, &mut checked)?;
    Ok(columns)
}

fn decode_column(
    reader: &mut Reader<'_>,
    column_index: usize,
    max_terms: usize,
    total_terms: &mut usize,
) -> CertificateResult<ChangeColumn> {
    let term_count = reader.usize()?;
    add_term_count(total_terms, term_count, max_terms)?;
    let bytes = term_count
        .checked_mul(12)
        .ok_or_else(|| CertificateError::new("term bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(CertificateError::new(format!(
            "column {column_index} terms exceed the remaining bytes"
        )));
    }
    let mut terms = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        terms.push(decode_term(reader)?);
    }
    Ok(ChangeColumn { terms })
}

fn decode_term(reader: &mut Reader<'_>) -> CertificateResult<CertificateTerm> {
    Ok(CertificateTerm {
        index: reader.usize()?,
        coefficient: reader.u32()?,
    })
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), CertificateError> {
    let value = u64::try_from(value)
        .map_err(|_| CertificateError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], CertificateError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| CertificateError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(CertificateError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, CertificateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, CertificateError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> std::result::Result<u32, CertificateError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, CertificateError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, CertificateError> {
        usize::try_from(self.u64()?)
            .map_err(|_| CertificateError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, CertificateError> {
        let value = self.usize()?;
        if value > limit {
            return Err(CertificateError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_f64(&mut self) -> std::result::Result<Option<f64>, CertificateError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(CertificateError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], CertificateError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

#[derive(Debug, Clone, Copy)]
struct FilteredEdge {
    vertices: [usize; 2],
    value: f64,
}

#[derive(Debug, Clone, Copy)]
struct FilteredTriangle {
    vertices: [usize; 3],
    value: f64,
}

struct FilteredComplex {
    vertex_count: usize,
    edges: Vec<FilteredEdge>,
    triangles: Vec<FilteredTriangle>,
    edge_rows: FxHashMap<(usize, usize), usize>,
}

impl FilteredComplex {
    fn build(
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

    fn edge_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
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

    fn triangle_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SparseColumn(BTreeMap<usize, u64>);

impl SparseColumn {
    fn insert(&mut self, index: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(index, coefficient);
        }
    }

    fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&index, &value)| (index, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&index, &value) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }
}

fn reduce_with_basis(
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    reduce_with_prefix(boundaries, &[], modulus, limits).map(|(columns, _)| columns)
}

fn reduce_with_prefix(
    boundaries: &[SparseColumn],
    prefix: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<(Vec<ChangeColumn>, usize), CertificateError> {
    if prefix.len() > boundaries.len() {
        return Err(CertificateError::new(
            "reduction prefix is longer than the boundary matrix",
        ));
    }
    let mut state = ReductionState::new(boundaries.len(), modulus as u64);
    for (index, transform) in prefix.iter().enumerate() {
        state.retain(index, transform, boundaries, limits.max_terms)?;
    }
    for (index, boundary) in boundaries.iter().enumerate().skip(prefix.len()) {
        state.reduce(index, boundary, limits.max_terms)?;
    }
    Ok(state.finish())
}

struct ReductionState {
    modulus: u64,
    reduced: Vec<SparseColumn>,
    basis: Vec<SparseColumn>,
    pivot_owner: FxHashMap<usize, usize>,
    total_terms: usize,
    additions: usize,
}

impl ReductionState {
    fn new(capacity: usize, modulus: u64) -> Self {
        Self {
            modulus,
            reduced: Vec::with_capacity(capacity),
            basis: Vec::with_capacity(capacity),
            pivot_owner: FxHashMap::default(),
            total_terms: 0,
            additions: 0,
        }
    }

    fn retain(
        &mut self,
        index: usize,
        transform: &ChangeColumn,
        boundaries: &[SparseColumn],
        max_terms: usize,
    ) -> CertificateResult<()> {
        let (column, basis_column) = retained_column(index, transform, boundaries, self.modulus)?;
        if let Some((pivot, _)) = column.pivot() {
            if self.pivot_owner.insert(pivot, index).is_some() {
                return Err(CertificateError::new(
                    "retained reduction prefix has duplicate pivots",
                ));
            }
        }
        self.add_terms(basis_column.0.len(), max_terms)?;
        self.reduced.push(column);
        self.basis.push(basis_column);
        Ok(())
    }

    fn reduce(
        &mut self,
        index: usize,
        boundary: &SparseColumn,
        max_terms: usize,
    ) -> CertificateResult<()> {
        let mut column = boundary.clone();
        let mut transform = SparseColumn::default();
        transform.insert(index, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = self.pivot_owner.get(&pivot) else {
                break;
            };
            let owner_coefficient = self.reduced[owner]
                .pivot()
                .expect("pivot owner is nonempty")
                .1;
            let factor = (self.modulus
                - coefficient * inverse_mod(owner_coefficient, self.modulus) % self.modulus)
                % self.modulus;
            column.add_scaled(&self.reduced[owner], factor, self.modulus);
            transform.add_scaled(&self.basis[owner], factor, self.modulus);
            self.additions = self
                .additions
                .checked_add(1)
                .ok_or_else(|| CertificateError::new("column addition count overflows usize"))?;
        }
        if let Some((pivot, _)) = column.pivot() {
            self.pivot_owner.insert(pivot, index);
        }
        self.add_terms(transform.0.len(), max_terms)?;
        self.reduced.push(column);
        self.basis.push(transform);
        Ok(())
    }

    fn add_terms(&mut self, count: usize, maximum: usize) -> CertificateResult<()> {
        self.total_terms = self
            .total_terms
            .checked_add(count)
            .ok_or_else(|| CertificateError::new("change-of-basis term count overflows usize"))?;
        if self.total_terms > maximum {
            return Err(CertificateError::new(format!(
                "{} change-of-basis terms exceed the limit {maximum}",
                self.total_terms
            )));
        }
        Ok(())
    }

    fn finish(self) -> (Vec<ChangeColumn>, usize) {
        let columns = self
            .basis
            .into_iter()
            .map(change_column_from_sparse)
            .collect();
        (columns, self.additions)
    }
}

fn retained_column(
    index: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> CertificateResult<(SparseColumn, SparseColumn)> {
    let mut column = SparseColumn::default();
    let mut basis_column = SparseColumn::default();
    let mut previous = None;
    for term in &transform.terms {
        check_retained_term(index, term, previous, modulus)?;
        column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        basis_column.insert(term.index, term.coefficient as u64);
        previous = Some(term.index);
    }
    if basis_column.0.get(&index) != Some(&1) {
        return Err(CertificateError::new(
            "retained reduction prefix is not unit triangular",
        ));
    }
    Ok((column, basis_column))
}

fn check_retained_term(
    target: usize,
    term: &CertificateTerm,
    previous: Option<usize>,
    modulus: u64,
) -> CertificateResult<()> {
    if term.index > target || previous.is_some_and(|value| value >= term.index) {
        return Err(CertificateError::new(
            "retained reduction prefix is not unit triangular",
        ));
    }
    if term.coefficient == 0 || term.coefficient as u64 >= modulus {
        return Err(CertificateError::new(
            "retained reduction prefix has an invalid coefficient",
        ));
    }
    Ok(())
}

fn change_column_from_sparse(column: SparseColumn) -> ChangeColumn {
    ChangeColumn {
        terms: column
            .0
            .into_iter()
            .map(|(index, coefficient)| CertificateTerm {
                index,
                coefficient: coefficient as u32,
            })
            .collect(),
    }
}

fn reindexed_prefix_candidates<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    check_simplex_counts(old_simplices, new_simplices, old_columns, "repair")?;
    let old_positions = simplex_positions(old_simplices);
    let new_positions = simplex_positions(new_simplices);
    check_repair_simplex_sets(old_simplices, new_simplices, &old_positions, &new_positions)?;
    let mut columns = Vec::new();
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[simplex];
        let Some(column) = reindexed_prefix_column(
            new_target,
            old_simplices,
            &new_positions,
            &old_columns[old_target],
        )?
        else {
            break;
        };
        columns.push(column);
    }
    Ok(columns)
}

fn simplex_positions<const N: usize>(simplices: &[[usize; N]]) -> BTreeMap<[usize; N], usize> {
    simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect()
}

fn check_simplex_counts<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
    operation: &str,
) -> CertificateResult<()> {
    if old_simplices.len() != new_simplices.len() || old_columns.len() != old_simplices.len() {
        return Err(CertificateError::new(format!(
            "reduction {operation} has inconsistent simplex counts"
        )));
    }
    Ok(())
}

fn check_repair_simplex_sets<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_positions: &BTreeMap<[usize; N], usize>,
    new_positions: &BTreeMap<[usize; N], usize>,
) -> CertificateResult<()> {
    if old_positions.len() != old_simplices.len()
        || new_positions.len() != new_simplices.len()
        || old_positions.keys().ne(new_positions.keys())
    {
        return Err(CertificateError::new(
            "reduction repair found a changed simplex set",
        ));
    }
    Ok(())
}

fn reindexed_prefix_column<const N: usize>(
    new_target: usize,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
    old_column: &ChangeColumn,
) -> CertificateResult<Option<ChangeColumn>> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = old_simplices.get(term.index).ok_or_else(|| {
            CertificateError::new("reduction repair found an invalid source position")
        })?;
        let index = new_positions[source];
        if index > new_target {
            return Ok(None);
        }
        terms.push(CertificateTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_unstable();
    check_distinct_terms(&terms, "reduction repair")?;
    Ok(Some(ChangeColumn { terms }))
}

fn check_distinct_terms(terms: &[CertificateTerm], operation: &str) -> CertificateResult<()> {
    if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err(CertificateError::new(format!(
            "{operation} produced duplicate source positions"
        )));
    }
    Ok(())
}

fn valid_reduction_prefix_len(
    boundaries: &[SparseColumn],
    candidates: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<usize, CertificateError> {
    let modulus = modulus as u64;
    let mut pivots = FxHashMap::<usize, usize>::default();
    let mut total_terms = 0usize;
    for (target, transform) in candidates.iter().enumerate() {
        let Some(reduced) = candidate_reduction(target, transform, boundaries, modulus)? else {
            return Ok(target);
        };
        if reduced
            .pivot()
            .is_some_and(|(pivot, _)| pivots.insert(pivot, target).is_some())
        {
            return Ok(target);
        }
        total_terms = total_terms
            .checked_add(transform.terms.len())
            .ok_or_else(|| CertificateError::new("change-of-basis term count overflows usize"))?;
        if total_terms > limits.max_terms {
            return Err(CertificateError::new(format!(
                "{total_terms} change-of-basis terms exceed the limit {}",
                limits.max_terms
            )));
        }
    }
    Ok(candidates.len())
}

fn candidate_reduction(
    target: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> CertificateResult<Option<SparseColumn>> {
    let mut reduced = SparseColumn::default();
    let mut previous = None;
    for term in &transform.terms {
        if term.index > target || previous.is_some_and(|value| value >= term.index) {
            return Ok(None);
        }
        if term.coefficient == 0 || term.coefficient as u64 >= modulus {
            return Err(CertificateError::new(
                "reduction repair found an invalid coefficient",
            ));
        }
        reduced.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        previous = Some(term.index);
    }
    let unit_diagonal = transform
        .terms
        .last()
        .is_some_and(|term| (term.index, term.coefficient) == (target, 1));
    Ok(unit_diagonal.then_some(reduced))
}

fn reindex_change_columns<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    check_simplex_counts(old_simplices, new_simplices, old_columns, "reindexing")?;
    let old_positions = simplex_positions(old_simplices);
    let new_positions = simplex_positions(new_simplices);
    if old_positions.len() != old_simplices.len() || new_positions.len() != new_simplices.len() {
        return Err(CertificateError::new(
            "reduction reindexing found duplicate simplex identities",
        ));
    }
    let mut columns = Vec::with_capacity(new_simplices.len());
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions.get(simplex).copied().ok_or_else(|| {
            CertificateError::new("reduction reindexing found a changed simplex set")
        })?;
        columns.push(reindex_column(
            new_target,
            old_simplices,
            &new_positions,
            &old_columns[old_target],
        )?);
    }
    Ok(columns)
}

fn reindex_column<const N: usize>(
    new_target: usize,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
    old_column: &ChangeColumn,
) -> CertificateResult<ChangeColumn> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = old_simplices.get(term.index).ok_or_else(|| {
            CertificateError::new("reduction reindexing found an invalid source position")
        })?;
        let index = new_positions.get(source).copied().ok_or_else(|| {
            CertificateError::new("reduction reindexing found a changed simplex set")
        })?;
        if index > new_target {
            return Err(CertificateError::new(
                "accepted reduction is not filtration-compatible after reindexing",
            ));
        }
        terms.push(CertificateTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_unstable();
    check_distinct_terms(&terms, "reduction reindexing")?;
    Ok(ChangeColumn { terms })
}

struct CheckedReductions {
    diagram: Diagram,
    h1_pairs: Vec<(Bar, CriticalPair)>,
    reduced_edges: Vec<SparseColumn>,
    reduced_triangles: Vec<SparseColumn>,
}

fn check_reductions(
    complex: &FilteredComplex,
    modulus: u32,
    edge_columns: &[ChangeColumn],
    triangle_columns: &[ChangeColumn],
    limits: CertificateLimits,
) -> std::result::Result<CheckedReductions, CertificateError> {
    let edge_boundaries = complex.edge_boundaries(modulus);
    let triangle_boundaries = complex.triangle_boundaries(modulus);
    let reduced_edges = check_matrix("edge", &edge_boundaries, edge_columns, modulus, limits)?;
    let reduced_triangles = check_matrix(
        "triangle",
        &triangle_boundaries,
        triangle_columns,
        modulus,
        limits,
    )?;
    let mut diagram = checked_h0_diagram(complex, &reduced_edges);
    let h1_deaths = h1_death_simplices(complex, &reduced_triangles);
    let h1_pairs = checked_h1_pairs(complex, &reduced_edges, &h1_deaths, &mut diagram);
    diagram.canonicalize();
    let mut h1_pairs = h1_pairs;
    h1_pairs.sort_by(critical_pair_record_order);
    Ok(CheckedReductions {
        diagram,
        h1_pairs,
        reduced_edges,
        reduced_triangles,
    })
}

fn checked_h0_diagram(complex: &FilteredComplex, reduced_edges: &[SparseColumn]) -> Diagram {
    let mut diagram = Diagram::default();
    let mut killed_vertices = vec![false; complex.vertex_count];
    for (column, reduced) in reduced_edges.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            killed_vertices[pivot] = true;
            let death = complex.edges[column].value;
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
    }
    for killed in killed_vertices {
        if !killed {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
    }
    diagram
}

fn h1_death_simplices<'a>(
    complex: &'a FilteredComplex,
    reduced_triangles: &[SparseColumn],
) -> FxHashMap<usize, &'a FilteredTriangle> {
    let mut h1_deaths = FxHashMap::default();
    for (column, reduced) in reduced_triangles.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            h1_deaths.insert(pivot, &complex.triangles[column]);
        }
    }
    h1_deaths
}

fn checked_h1_pairs(
    complex: &FilteredComplex,
    reduced_edges: &[SparseColumn],
    h1_deaths: &FxHashMap<usize, &FilteredTriangle>,
    diagram: &mut Diagram,
) -> Vec<(Bar, CriticalPair)> {
    let mut h1_pairs = Vec::new();
    for (edge, reduced) in reduced_edges.iter().enumerate() {
        if !reduced.0.is_empty() {
            continue;
        }
        let birth = complex.edges[edge].value;
        let death_simplex = h1_deaths.get(&edge).copied();
        let death = death_simplex.map_or(f64::INFINITY, |triangle| triangle.value);
        if death > birth {
            let interval = Bar {
                dim: 1,
                birth,
                death,
            };
            diagram.bars.push(interval);
            h1_pairs.push((
                interval,
                CriticalPair {
                    birth: CriticalSimplex {
                        vertices: complex.edges[edge].vertices.to_vec(),
                        value: birth,
                    },
                    death: death_simplex.map(|triangle| CriticalSimplex {
                        vertices: triangle.vertices.to_vec(),
                        value: triangle.value,
                    }),
                },
            ));
        }
    }
    h1_pairs
}

fn check_matrix(
    label: &str,
    boundaries: &[SparseColumn],
    columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<Vec<SparseColumn>, CertificateError> {
    if columns.len() != boundaries.len() {
        return Err(CertificateError::new(format!(
            "{label} matrix has {} columns, certificate records {}",
            boundaries.len(),
            columns.len()
        )));
    }
    let modulus64 = modulus as u64;
    let mut total_terms = 0usize;
    let mut reduced = Vec::with_capacity(columns.len());
    let mut pivots = FxHashMap::default();
    for (column_index, transform) in columns.iter().enumerate() {
        add_matrix_terms(
            &mut total_terms,
            transform.terms.len(),
            limits.max_terms,
            label,
        )?;
        let result = checked_matrix_column(
            label,
            column_index,
            transform,
            boundaries,
            modulus,
            modulus64,
        )?;
        if let Some((pivot, _)) = result.pivot() {
            if let Some(previous_column) = pivots.insert(pivot, column_index) {
                return Err(CertificateError::new(format!(
                    "{label} reduced columns {previous_column} and {column_index} share pivot {pivot}"
                )));
            }
        }
        reduced.push(result);
    }
    Ok(reduced)
}

fn add_matrix_terms(
    total: &mut usize,
    count: usize,
    maximum: usize,
    label: &str,
) -> CertificateResult<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("certificate term count overflows usize"))?;
    if *total > maximum {
        return Err(CertificateError::new(format!(
            "{} {label} terms exceed the limit {maximum}",
            *total
        )));
    }
    Ok(())
}

fn checked_matrix_column(
    label: &str,
    column_index: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u32,
    modulus64: u64,
) -> CertificateResult<SparseColumn> {
    check_change_column(label, column_index, transform, modulus)?;
    let mut result = SparseColumn::default();
    for term in &transform.terms {
        result.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
    }
    Ok(result)
}

fn add_change_guards(
    guards: &mut BTreeSet<ReductionGuard>,
    columns: &[ChangeColumn],
    simplices: &[FiltrationSimplex],
) {
    for (target, column) in columns.iter().enumerate() {
        for term in &column.terms {
            if term.index != target {
                guards.insert(ReductionGuard {
                    kind: ReductionGuardKind::ChangeOfBasis,
                    earlier: simplices[term.index].clone(),
                    later: simplices[target].clone(),
                });
            }
        }
    }
}

fn add_pivot_guards(
    guards: &mut BTreeSet<ReductionGuard>,
    reduced: &[SparseColumn],
    row_simplices: &[FiltrationSimplex],
) {
    for column in reduced {
        let Some((pivot, _)) = column.pivot() else {
            continue;
        };
        for &row in column.0.keys() {
            if row != pivot {
                guards.insert(ReductionGuard {
                    kind: ReductionGuardKind::Pivot,
                    earlier: row_simplices[row].clone(),
                    later: row_simplices[pivot].clone(),
                });
            }
        }
    }
}

fn minimize_guards(guards: BTreeSet<ReductionGuard>) -> Vec<ReductionGuard> {
    let mut pairs = BTreeMap::new();
    for guard in guards {
        pairs
            .entry((guard.earlier, guard.later))
            .and_modify(|kind: &mut ReductionGuardKind| *kind = (*kind).min(guard.kind))
            .or_insert(guard.kind);
    }
    let mut nodes = BTreeMap::new();
    for (earlier, later) in pairs.keys() {
        for simplex in [earlier, later] {
            let next = nodes.len();
            nodes.entry(simplex.clone()).or_insert(next);
        }
    }
    let edges: Vec<_> = pairs
        .into_iter()
        .map(|((earlier, later), kind)| {
            let source = nodes[&earlier];
            let target = nodes[&later];
            (
                source,
                target,
                ReductionGuard {
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
    let mut reduced = BTreeSet::new();
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
            reduced.insert(guard);
        }
    }
    reduced.into_iter().collect()
}

fn region_value_formula(
    simplex: &FiltrationSimplex,
    edge_indices: &BTreeMap<[usize; 2], usize>,
) -> RegionValueFormula {
    match *simplex.vertices.as_slice() {
        [_] => RegionValueFormula::Vertex,
        [u, v] => RegionValueFormula::Edge(edge_indices[&[u, v]]),
        [u, v, w] => RegionValueFormula::Triangle([
            edge_indices[&[u, v]],
            edge_indices[&[u, w]],
            edge_indices[&[v, w]],
        ]),
        _ => unreachable!("certified regions contain vertices, edges, and triangles"),
    }
}

fn simplex_rank(simplex: &FiltrationSimplex) -> u128 {
    match *simplex.vertices.as_slice() {
        [u] => u as u128,
        [u, v] => edge_rank([u, v]),
        [u, v, w] => triangle_rank([u, v, w]),
        _ => unreachable!("certified regions contain vertices, edges, and triangles"),
    }
}

fn triangle_value(matrix: &SparseDistanceMatrix, [u, v, w]: [usize; 3]) -> f64 {
    matrix.get(u, v).max(matrix.get(u, w)).max(matrix.get(v, w))
}

fn critical_pair_record_order(
    a: &(Bar, CriticalPair),
    b: &(Bar, CriticalPair),
) -> std::cmp::Ordering {
    a.0.birth
        .total_cmp(&b.0.birth)
        .then(a.0.death.total_cmp(&b.0.death))
        .then(a.1.birth.vertices.cmp(&b.1.birth.vertices))
        .then_with(|| {
            a.1.death
                .as_ref()
                .map(|simplex| &simplex.vertices)
                .cmp(&b.1.death.as_ref().map(|simplex| &simplex.vertices))
        })
}

fn validate_header(
    input: &SparseDistanceMatrix,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<(), CertificateError> {
    if max_dim != 1 {
        return Err(CertificateError::new(
            "an algebraic certificate requires max_dim equal to 1",
        ));
    }
    if input.len() > limits.max_vertices {
        return Err(CertificateError::new(format!(
            "{} vertices exceed the limit {}",
            input.len(),
            limits.max_vertices
        )));
    }
    if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
        return Err(CertificateError::new(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )));
    }
    Ok(())
}

fn checked_threshold(threshold: Option<f64>) -> std::result::Result<f64, CertificateError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(CertificateError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn graph_digest(input: &SparseDistanceMatrix, threshold: f64) -> [u8; 32] {
    let edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((input.len() as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for (u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

fn triangle_rank([u, v, w]: [usize; 3]) -> u128 {
    let choose2 = v as u128 * (v.saturating_sub(1)) as u128 / 2;
    let choose3 = w as u128 * (w.saturating_sub(1)) as u128 * (w.saturating_sub(2)) as u128 / 6;
    u as u128 + choose2 + choose3
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

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

impl From<CertificateError> for Error {
    fn from(error: CertificateError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
    fn square_certificate_verifies_over_several_fields() {
        let input = square();
        for modulus in [2, 3, 5, 7] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let certificate =
                ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
            let bytes = certificate.encode().unwrap();
            let certificate =
                ReductionCertificate::decode(&bytes, CertificateLimits::default()).unwrap();
            assert_eq!(certificate.encode().unwrap(), bytes);
            let diagram = certificate
                .verify(&input, CertificateLimits::default())
                .unwrap();
            let expected = rips_persistence_sparse(&input, &params).unwrap();
            assert!(diagram_bits_equal(&diagram, &expected));
        }
    }

    #[test]
    fn repair_retains_the_stable_reduction_prefix() {
        let current = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 2.0),
                (0, 3, 3.0),
                (1, 2, 4.0),
                (1, 3, 5.0),
                (2, 3, 6.0),
            ],
        )
        .unwrap();
        let updated = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 2.0),
                (0, 3, 3.0),
                (1, 2, 4.0),
                (1, 3, 6.5),
                (2, 3, 6.0),
            ],
        )
        .unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let certificate =
                ReductionCertificate::build(&current, &params, CertificateLimits::default())
                    .unwrap();
            let repair = certificate
                .repair(&current, &updated, CertificateLimits::default())
                .unwrap();
            assert_eq!(repair.mode(), ReductionRepairMode::SuffixRepaired);
            assert_eq!(repair.work().edge_columns_reused, 6);
            assert_eq!(repair.work().edge_columns_reduced, 0);
            assert!(repair.work().triangle_columns_reused > 0);
            assert!(repair.work().triangle_columns_reduced > 0);
            let actual = repair
                .certificate()
                .verify(&updated, CertificateLimits::default())
                .unwrap();
            let expected = rips_persistence_sparse(&updated, &params).unwrap();
            assert!(diagram_bits_equal(&actual, &expected));
        }
    }

    #[test]
    fn repair_rejects_topology_and_threshold_changes() {
        let current = square();
        let params = RipsParams::new(1).with_threshold(1.5);
        let certificate =
            ReductionCertificate::build(&current, &params, CertificateLimits::default()).unwrap();
        let topology = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
            ],
        )
        .unwrap();
        assert!(
            certificate
                .repair(&current, &topology, CertificateLimits::default())
                .is_err()
        );
        let crossing = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 2.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap();
        assert!(
            certificate
                .repair(&current, &crossing, CertificateLimits::default())
                .is_err()
        );
    }

    #[test]
    fn dependency_frontier_repairs_random_edge_swaps_over_prime_fields() {
        let mut state = 0x71f3_2c85_9a40_b6d1u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut saw_reuse = false;
        for case in 0..24 {
            let n = 5 + next() as usize % 4;
            let mut endpoints = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    endpoints.push((u, v));
                }
            }
            let mut order: Vec<_> = (0..endpoints.len()).map(|index| (next(), index)).collect();
            order.sort_unstable();
            let mut rank = vec![0usize; endpoints.len()];
            for (position, &(_, index)) in order.iter().enumerate() {
                rank[index] = position;
            }
            let triplets: Vec<_> = endpoints
                .iter()
                .enumerate()
                .map(|(index, &(u, v))| (u, v, 1.0 + rank[index] as f64))
                .collect();
            let left = endpoints.len() - 2 - next() as usize % 3.min(endpoints.len() - 1);
            let right = left + 1;
            let mut changed = triplets.clone();
            changed[left].2 = triplets[right].2;
            changed[right].2 = triplets[left].2;
            let current = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            let updated = SparseDistanceMatrix::from_triplets(n, &changed).unwrap();
            for modulus in [2, 3, 5] {
                let params = RipsParams::new(1).with_modulus(modulus);
                let certificate =
                    ReductionCertificate::build(&current, &params, CertificateLimits::default())
                        .unwrap_or_else(|error| panic!("case {case}, Z/{modulus}: {error}"));
                let repair = certificate
                    .repair(&current, &updated, CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, Z/{modulus}: {error}"));
                saw_reuse |= repair.work().columns_reused() > 0;
                let actual = repair
                    .certificate()
                    .verify(&updated, CertificateLimits::default())
                    .unwrap();
                let expected = rips_persistence_sparse(&updated, &params).unwrap();
                assert!(diagram_bits_equal(&actual, &expected));
            }
        }
        assert!(saw_reuse);
    }

    #[test]
    fn changed_basis_term_and_graph_are_rejected() {
        let input = square();
        let params = RipsParams::new(1).with_modulus(3);
        let mut certificate =
            ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
        certificate.triangle_columns[0].terms[0].coefficient = 0;
        assert!(
            certificate
                .verify(&input, CertificateLimits::default())
                .is_err()
        );

        let other =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap();
        assert!(
            ReductionCertificate::build(&input, &params, CertificateLimits::default())
                .unwrap()
                .verify(&other, CertificateLimits::default())
                .is_err()
        );
    }

    #[test]
    fn random_certificates_match_the_implicit_solver() {
        let mut state = 0x57c8_02ed_4a91_b36fu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..96 {
            let n = 3 + next() as usize % 8;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 3 {
                        triplets.push((u, v, (next() % 6) as f64));
                    }
                }
            }
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            for modulus in [2, 3, 5] {
                let params = RipsParams::new(1)
                    .with_modulus(modulus)
                    .with_threshold((next() % 7) as f64);
                let certificate =
                    ReductionCertificate::build(&input, &params, CertificateLimits::default())
                        .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
                certificate
                    .verify(&input, CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            }
        }
    }

    #[test]
    fn certified_region_survives_an_irrelevant_edge_swap() {
        let triplets = vec![
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (4, 5, 5.0),
            (5, 6, 6.0),
            (6, 7, 7.0),
            (4, 7, 8.0),
        ];
        let input = SparseDistanceMatrix::from_triplets(8, &triplets).unwrap();
        let params = RipsParams::new(1).with_modulus(3);
        let certificate =
            ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
        let region = certificate
            .compile_region(&input, CertificateLimits::default())
            .unwrap();
        let atlas = crate::PersistenceAtlas::build(&input, &params).unwrap();
        let mut accepted = None;
        for first in 0..triplets.len() {
            for second in first + 1..triplets.len() {
                let mut updated_triplets = triplets.clone();
                let first_value = updated_triplets[first].2;
                updated_triplets[first].2 = updated_triplets[second].2;
                updated_triplets[second].2 = first_value;
                let updated = SparseDistanceMatrix::from_triplets(8, &updated_triplets).unwrap();
                if !atlas.events(&updated).is_empty() && region.violations(&updated).is_empty() {
                    accepted = Some(updated);
                    break;
                }
            }
            if accepted.is_some() {
                break;
            }
        }
        let updated = accepted.expect("one cross-component order swap is algebraically irrelevant");
        let evaluation = region.evaluate(&updated).unwrap();
        let expected = rips_persistence_sparse(&updated, &params).unwrap();
        assert!(diagram_bits_equal(evaluation.diagram(), &expected));
        let rebound = certificate
            .reindex(&input, &updated, CertificateLimits::default())
            .unwrap();
        let checked = rebound
            .verify(&updated, CertificateLimits::default())
            .unwrap();
        assert!(diagram_bits_equal(&checked, &expected));
        assert!(!region.guards().is_empty());
    }

    #[test]
    fn random_accepted_reweightings_match_exact_reduction() {
        let mut state = 0xa314_56f0_7c2d_98ebu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut accepted = 0usize;
        for case in 0..48 {
            let n = 5 + next() as usize % 6;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 3 {
                        triplets.push((u, v, (1 + next() % 20) as f64));
                    }
                }
            }
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            for modulus in [2, 3, 5] {
                let params = RipsParams::new(1).with_modulus(modulus);
                let certificate =
                    ReductionCertificate::build(&input, &params, CertificateLimits::default())
                        .unwrap();
                let region = certificate
                    .compile_region(&input, CertificateLimits::default())
                    .unwrap();
                for attempt in 0..8 {
                    let updated_triplets: Vec<_> = triplets
                        .iter()
                        .map(|&(u, v, value)| {
                            let delta = (next() % 7) as f64 * 0.01 * (attempt + 1) as f64;
                            (u, v, value + delta)
                        })
                        .collect();
                    let updated =
                        SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
                    if region.violations(&updated).is_empty() {
                        accepted += 1;
                        let actual = region.evaluate(&updated).unwrap();
                        let expected = rips_persistence_sparse(&updated, &params).unwrap();
                        assert!(
                            diagram_bits_equal(actual.diagram(), &expected),
                            "case {case}, modulus {modulus}, attempt {attempt}"
                        );
                    }
                }
            }
        }
        assert!(accepted > 100);
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = ReductionCertificate::decode(&bytes, CertificateLimits::default());
        }
    }
}
