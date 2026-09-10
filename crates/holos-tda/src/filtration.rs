//! Explicit filtered simplicial complexes and filtration grades.
//!
//! Relative interfaces and explicit certificates take
//! [`FilteredSimplicialComplex`] values.

mod flag;
mod grade;
mod simplex;

pub use flag::{ComplexLimits, FlagComplexParams};
pub use grade::{
    CoordinateProjection, FiltrationError, FiltrationGrade, LinearFiltrationGrade, ProductGrade,
    ScalarGrade, ScalarProjection,
};
pub use simplex::{FilteredSimplex, FilteredSimplicialComplex};

#[cfg(test)]
mod tests;
