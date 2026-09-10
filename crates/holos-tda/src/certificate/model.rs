//! Public certificate data types and shared private models.

use std::fmt;

use crate::{Bar, CriticalPair, Diagram, Error};

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
    pub(super) certificate: ReductionCertificate,
    pub(super) mode: ReductionRepairMode,
    pub(super) work: ReductionRepairWork,
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
    pub(super) vertices: Vec<usize>,
}

impl FiltrationSimplex {
    pub(super) fn new(vertices: impl Into<Vec<usize>>) -> Self {
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
    pub(super) kind: ReductionGuardKind,
    pub(super) earlier: FiltrationSimplex,
    pub(super) later: FiltrationSimplex,
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
    pub(super) kind: RegionViolationKind,
    pub(super) guard_index: Option<usize>,
    pub(super) first: Option<FiltrationSimplex>,
    pub(super) second: Option<FiltrationSimplex>,
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
    pub(super) diagram: Diagram,
    pub(super) h1_pairs: Vec<(Bar, CriticalPair)>,
    pub(super) guards_checked: usize,
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
pub(super) struct RegionH1Pair {
    pub(super) birth: [usize; 2],
    pub(super) death: Option<[usize; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum RegionValueFormula {
    Vertex,
    Edge(usize),
    Triangle([usize; 3]),
}

impl RegionValueFormula {
    pub(super) fn value(self, edge_value: impl Fn(usize) -> f64) -> f64 {
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
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) topology: Vec<[usize; 2]>,
    pub(super) active: Vec<bool>,
    pub(super) complete_guards: Vec<ReductionGuard>,
    pub(super) guards: Vec<ReductionGuard>,
    pub(super) guard_indices: Vec<(usize, usize)>,
    pub(super) guard_ranks: Vec<u128>,
    pub(super) guard_formulas: Vec<RegionValueFormula>,
    pub(super) h0_deaths: Vec<[usize; 2]>,
    pub(super) h0_essential: usize,
    pub(super) h1_pairs: Vec<RegionH1Pair>,
    pub(super) h1_formulas: Vec<(usize, Option<[usize; 3]>)>,
}

/// Proof that a filtered boundary matrix has the declared reduced pivots.
#[derive(Debug, Clone)]
pub struct ReductionCertificate {
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) modulus: u32,
    pub(super) graph_digest: [u8; 32],
    pub(super) edge_columns: Vec<ChangeColumn>,
    pub(super) triangle_columns: Vec<ChangeColumn>,
    pub(super) diagram: Diagram,
}

pub(super) type CertificateResult<T> = std::result::Result<T, CertificateError>;
impl From<CertificateError> for Error {
    fn from(error: CertificateError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}
