//! Algebraic certificates for exact H0 and H1 persistence.
//!
//! The producer reduces explicit edge and triangle boundary matrices and
//! records their sparse change-of-basis columns. The checker does not call
//! the holos persistence solver. It reconstructs each original boundary,
//! checks the declared change of basis, requires distinct reduced pivots,
//! and derives the diagram from the checked columns.

mod build;
mod cycles;
mod model;
mod reduction;
mod region;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    CertificateError, CertificateLimits, CertificateTerm, CertifiedReductionRegion,
    CertifiedRegionEvaluation, ChangeColumn, FiltrationSimplex, ReductionCertificate,
    ReductionGuard, ReductionGuardKind, ReductionRepair, ReductionRepairMode, ReductionRepairWork,
    RegionViolation, RegionViolationKind,
};
