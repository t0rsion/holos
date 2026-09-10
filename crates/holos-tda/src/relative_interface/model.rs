use crate::Diagram;
use crate::certificate::ChangeColumn;

/// One nonzero term in a filtered cellular boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceChainTerm {
    /// Labeled vertices of the boundary cell.
    pub cell: Vec<usize>,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One labeled cell in a filtered relative interface.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCell {
    /// Labeled vertices in ascending order.
    pub vertices: Vec<usize>,
    /// Filtration value of this cell.
    pub value: f64,
    /// Sparse cellular boundary in ascending cell order.
    pub boundary: Vec<InterfaceChainTerm>,
}

impl InterfaceCell {
    /// Dimension of this cell.
    pub fn dimension(&self) -> usize {
        self.vertices.len() - 1
    }
}

/// One checked equal-filtration unit cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCancellation {
    /// Higher-dimensional cell removed by this step.
    pub upper: Vec<usize>,
    /// Codimension-one cell removed by this step.
    pub lower: Vec<usize>,
    /// Incidence coefficient before the cancellation.
    pub coefficient: u32,
}

/// Exact size and work of one relative interface certificate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelativeInterfaceWork {
    /// Cells before relative cancellation.
    pub input_cells: usize,
    /// Equal-filtration unit pairs removed.
    pub cancellations: usize,
    /// Cells in the retained core.
    pub core_cells: usize,
    /// Sparse additions used to reduce the retained core.
    pub reduction_additions: usize,
}

/// A filtered chain core relative to protected vertices.
///
/// The record includes its input chain complex, cancellation trace, retained
/// core, and one `D V = R` reduction per boundary dimension.
#[derive(Debug, Clone)]
pub struct RelativeInterfaceCertificate {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) protected_vertices: Vec<usize>,
    pub(super) input_cells: Vec<Vec<InterfaceCell>>,
    pub(super) cancellations: Vec<InterfaceCancellation>,
    pub(super) core_cells: Vec<Vec<InterfaceCell>>,
    pub(super) columns: Vec<Vec<ChangeColumn>>,
    pub(super) diagram: Diagram,
    pub(super) digest: [u8; 32],
    pub(super) work: RelativeInterfaceWork,
}
