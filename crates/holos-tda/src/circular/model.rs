use crate::classes::CocycleTerm;
use crate::cohomology::{CohomologyContinuation, CohomologyLimits, CohomologySpaceId};

/// Numerical and topological limits for one circular coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CircularCoordinateParams {
    /// Largest accepted relative normal-equation residual.
    pub tolerance: f64,
    /// Largest conjugate-gradient iteration count.
    pub max_iterations: usize,
    /// Limits for the canonical fixed-scale cohomology computation.
    pub cohomology: CohomologyLimits,
}

impl Default for CircularCoordinateParams {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            max_iterations: 10_000,
            cohomology: CohomologyLimits::default(),
        }
    }
}

impl CircularCoordinateParams {
    /// Set the largest accepted relative normal-equation residual.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Set the largest conjugate-gradient iteration count.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Set the fixed-scale cohomology limits.
    #[must_use]
    pub fn with_cohomology_limits(mut self, cohomology: CohomologyLimits) -> Self {
        self.cohomology = cohomology;
        self
    }
}

/// One nonzero coefficient of an integer cocycle on an oriented edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntegralCocycleTerm {
    /// Lower endpoint. The edge is oriented from `u` to `v`.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
    /// Nonzero integer coefficient.
    pub coefficient: i64,
}

/// One nonzero coordinate in a canonical fixed-scale H1 basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CircularClassTerm {
    /// Position in the canonical cohomology basis.
    pub basis_index: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One checked harmonic circle-valued coordinate on graph vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCoordinate {
    /// Canonical fixed-scale cohomology space.
    pub space: CohomologySpaceId,
    /// Prime field of the selected source cocycle.
    pub modulus: u32,
    /// Fixed Rips scale.
    pub scale: f64,
    /// Multiplier applied before the finite-field cocycle was lifted.
    pub field_multiplier: u32,
    /// Selected class before the lift, in canonical quotient coordinates.
    pub class: Vec<CircularClassTerm>,
    /// Source cocycle before the lift, in ascending endpoint order.
    pub source: Vec<CocycleTerm>,
    /// Checked integer cocycle congruent to the multiplied source class.
    pub integral: Vec<IntegralCocycleTerm>,
    /// Divisibility of the integral cohomology class.
    pub divisibility: u64,
    /// Gauge-fixed real vertex potential.
    pub potential: Vec<f64>,
    /// Circle-valued vertex coordinate in `[0, 1)`.
    pub phase: Vec<f64>,
    /// Squared unweighted harmonic energy.
    pub energy: f64,
    /// Maximum absolute normal-equation residual.
    pub max_residual: f64,
    /// Maximum residual divided by the source infinity norm, floored at one.
    pub relative_residual: f64,
    /// Conjugate-gradient iterations used.
    pub iterations: usize,
    /// Residual tolerance required by the producer.
    pub tolerance: f64,
}

/// Conservative circular-coordinate continuation to one changed graph.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCoordinateContinuation {
    /// Exact fixed-scale class continuation.
    pub topology: CohomologyContinuation,
    /// New coordinate when the continuation is unique and nonzero.
    pub coordinate: Option<CircularCoordinate>,
}

/// One harmonic coordinate computed from a checked source without a basis.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SelectedCircularCoordinate {
    pub(crate) modulus: u32,
    pub(crate) scale: f64,
    pub(crate) field_multiplier: u32,
    pub(crate) integral: Vec<IntegralCocycleTerm>,
    pub(crate) divisibility: u64,
    pub(crate) potential: Vec<f64>,
    pub(crate) phase: Vec<f64>,
    pub(crate) energy: f64,
    pub(crate) max_residual: f64,
    pub(crate) relative_residual: f64,
    pub(crate) iterations: usize,
    pub(crate) tolerance: f64,
}
