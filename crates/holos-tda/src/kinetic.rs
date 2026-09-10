//! Exact event schedules for affine edge-weight trajectories.
//!
//! Input `f64` values are interpreted as exact dyadic rationals. Threshold
//! crossings and pairwise order swaps are solved over those rationals. Each
//! public event carries the smallest adjacent-`f64` interval around its exact
//! time.

mod arithmetic;
mod cohomology;
mod filtration;
mod model;
mod schedule;
#[cfg(test)]
mod tests;
mod zigzag;

pub use model::{
    KineticCohomologyEvent, KineticEdge, KineticEdgeKey, KineticEvent, KineticEventKind,
    KineticFiltration, KineticGraphState, KineticGraphStateKind, KineticLimits, KineticSchedule,
    KineticZigzag, KineticZigzagArrow, KineticZigzagNode, KineticZigzagNodeKind,
};
