use std::collections::BTreeMap;

use crate::Diagram;
use crate::certificate::{ChangeColumn, ReductionRepairMode};

/// Work retained and recomputed in one boundary dimension.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GradedDimensionWork {
    /// Dimension of the source simplices in this boundary matrix.
    pub simplex_dimension: usize,
    /// Change-of-basis columns retained without reduction.
    pub columns_reused: usize,
    /// Change-of-basis columns processed by reduction.
    pub columns_reduced: usize,
    /// Sparse reduced-column additions.
    pub column_additions: usize,
}

/// Exact work charged to a dimension-generic reduction repair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GradedReductionRepairWork {
    pub(super) dimensions: Vec<GradedDimensionWork>,
}

impl GradedReductionRepairWork {
    /// Per-dimension work in ascending simplex dimension.
    pub fn dimensions(&self) -> &[GradedDimensionWork] {
        &self.dimensions
    }

    /// Columns retained without reduction.
    pub fn columns_reused(&self) -> usize {
        self.dimensions.iter().map(|work| work.columns_reused).sum()
    }

    /// Columns processed by reduction.
    pub fn columns_reduced(&self) -> usize {
        self.dimensions
            .iter()
            .map(|work| work.columns_reduced)
            .sum()
    }

    /// Sparse reduced-column additions.
    pub fn column_additions(&self) -> usize {
        self.dimensions
            .iter()
            .map(|work| work.column_additions)
            .sum()
    }
}

/// A checked graded reduction adapted to a new filtration.
#[derive(Debug, Clone)]
pub struct GradedReductionRepair {
    pub(super) certificate: GradedReductionCertificate,
    pub(super) mode: ReductionRepairMode,
    pub(super) work: GradedReductionRepairWork,
}

impl GradedReductionRepair {
    /// Repaired certificate bound to the updated graph.
    pub fn certificate(&self) -> &GradedReductionCertificate {
        &self.certificate
    }

    /// Whether the repair reused, repaired, or rebuilt its columns.
    pub fn mode(&self) -> ReductionRepairMode {
        self.mode
    }

    /// Exact work charged by the repair.
    pub fn work(&self) -> &GradedReductionRepairWork {
        &self.work
    }

    pub(crate) fn into_certificate(self) -> GradedReductionCertificate {
        self.certificate
    }
}

/// A dimension-generic `D V = R` certificate for a filtered flag complex.
#[derive(Debug, Clone)]
pub struct GradedReductionCertificate {
    pub(super) vertex_count: usize,
    pub(super) max_dim: usize,
    pub(super) threshold: Option<f64>,
    pub(super) modulus: u32,
    pub(super) graph_digest: [u8; 32],
    pub(super) columns: Vec<Vec<ChangeColumn>>,
    pub(super) diagram: Diagram,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SimplexKey(pub(crate) Vec<usize>);

#[derive(Debug, Clone)]
pub(crate) struct FilteredSimplex {
    pub(crate) key: SimplexKey,
    pub(crate) value: f64,
}

pub(crate) struct GradedComplex {
    pub(super) simplices: Vec<Vec<FilteredSimplex>>,
    pub(super) rows: Vec<BTreeMap<SimplexKey, usize>>,
}
#[derive(Debug, Clone, Default)]
pub(super) struct SparseColumn(pub(super) BTreeMap<usize, u64>);
pub(crate) struct CheckedGraded {
    pub(crate) diagram: Diagram,
}
