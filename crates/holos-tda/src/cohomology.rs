//! Canonical cohomology spaces and exact relations in any bounded dimension.
//!
//! A space is the quotient of cocycles by coboundaries at one filtration
//! scale. Sparse row reduction gives a deterministic basis on labeled flag
//! simplices. Two spaces relate by restriction to the common active flag
//! subcomplex.

mod algebra;
mod api;
mod complex;
mod digest;
mod forest;
mod methods;
mod model;

#[cfg(test)]
mod forest_tests;
#[cfg(test)]
mod tests;

pub use api::{
    cohomology_continuation, cohomology_relation, cohomology_restriction, cohomology_space,
};
pub use model::{
    CochainTerm, CohomologyClass, CohomologyClassId, CohomologyContinuation,
    CohomologyContinuationKind, CohomologyLimits, CohomologyMapColumn, CohomologyMapTerm,
    CohomologyRelation, CohomologyRelationTerm, CohomologyRelationVector, CohomologyRestriction,
    CohomologySpace, CohomologySpaceId, CohomologySubspace, CohomologySubspaceGenerator,
    CohomologySubspaceTerm,
};
