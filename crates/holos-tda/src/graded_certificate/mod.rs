//! Dimension-generic algebraic certificates for separator interfaces.
//!
//! A graded certificate records one unit-triangular change of basis for each
//! boundary dimension through `max_dim + 1`. The verifier reconstructs the
//! filtered flag complex, checks every `D V = R` relation, and derives the
//! diagram from the checked pivots. This module does not assign canonical
//! identities to classes above H1.

mod api;
mod complex;
mod digest;
mod model;
mod reduction;
mod repair;
mod validation;

#[cfg(test)]
mod tests;

pub use model::{
    GradedDimensionWork, GradedReductionCertificate, GradedReductionRepair,
    GradedReductionRepairWork,
};

pub(crate) use model::GradedComplex;
pub(crate) use reduction::{check_all, reduce_all_dimensions};
