//! Finite H1 lifetime interventions.
//!
//! The query lowers independent edge weights in the declared destroyer
//! triangles of one finite class space. The complete space must die no later
//! than a target scale. `Optimal` is relative to the current checked
//! reduction and its fixed destroyer simplices.

use crate::Error;

type Result<T, E = Error> = std::result::Result<T, E>;

mod model;
mod primitives;
mod search;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    EdgeWeightEdit, H1Intervention, InterventionArtifact, InterventionBudget,
    InterventionDecodeLimits, InterventionError, InterventionStatus, VerifiedIntervention,
};
