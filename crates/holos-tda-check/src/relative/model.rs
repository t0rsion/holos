use std::collections::BTreeMap;

use super::super::{Graph, ProofBar, ProofColumn, ProofLimits};

/// Counts from one checked relative interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRelativeInterface {
    /// Content identifier of the retained core and reduction.
    pub digest: [u8; 32],
    /// Highest checked homology dimension.
    pub max_dim: usize,
    /// Cells before relative cancellation.
    pub input_cells: usize,
    /// Equal-filtration unit cancellations checked.
    pub cancellations: usize,
    /// Cells in the retained core.
    pub core_cells: usize,
    /// Change-of-basis columns checked.
    pub reduction_columns: usize,
    /// Diagram bars derived from the reduction.
    pub bars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Term {
    pub(super) cell: Vec<usize>,
    pub(super) coefficient: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cell {
    pub(super) vertices: Vec<usize>,
    pub(super) value: f64,
    pub(super) boundary: Vec<Term>,
}

#[derive(Debug, Clone)]
pub(super) struct Step {
    pub(super) upper: Vec<usize>,
    pub(super) lower: Vec<usize>,
    pub(super) coefficient: u32,
}

pub(crate) struct VerifiedCertificate {
    pub(crate) max_dim: usize,
    pub(crate) modulus: u32,
    pub(crate) protected_vertices: Vec<usize>,
    pub(super) input: Vec<Vec<Cell>>,
    pub(super) steps: Vec<Step>,
    pub(super) core: Vec<Vec<Cell>>,
    pub(crate) columns: Vec<Vec<ProofColumn>>,
    pub(crate) diagram: Vec<ProofBar>,
    pub(crate) digest: [u8; 32],
}

pub(super) type DimensionMap = BTreeMap<Vec<usize>, Cell>;

pub(super) struct CertificateHeader {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) protected_vertices: Vec<usize>,
}

pub(crate) struct IndexLeafContext<'a> {
    pub(crate) graph: &'a Graph,
    pub(crate) labels: &'a [usize],
    pub(crate) threshold: Option<f64>,
    pub(crate) max_dim: usize,
    pub(crate) modulus: u32,
    pub(crate) protected_vertices: &'a [usize],
    pub(crate) limits: ProofLimits,
}
