//! Exact relative coverage witnesses for fenced planar Rips complexes.
//!
//! The algebraic criterion follows the controlled-boundary theorem of de
//! Silva and Ghrist. A nonzero fence cycle must bound a two-chain in the
//! active Rips complex. Physical coverage of the domain depends on
//! [`PlanarCoverageModel`].

mod algebra;
mod model;

pub use algebra::evaluate_planar_coverage;
pub use model::{
    CoverageEvaluation, CoverageFence, CoverageLimits, CoverageTriangleTerm, PlanarCoverageModel,
};

#[cfg(test)]
mod tests;
