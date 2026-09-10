//! Certified local models of H1 persistence under changing edge weights.
//!
//! An atlas fixes the vertex set, edge set, threshold membership, and weak
//! order of all listed edge weights. Within that region, every Rips simplex
//! keeps its filtration position. The persistence pairing, class-space basis,
//! and critical simplices stay fixed. Evaluation updates endpoint values
//! without another persistence reduction.

mod model;
mod persistence;
mod point;
mod support;
#[cfg(test)]
mod tests;

pub use model::{
    AtlasEvaluation, AtlasUpdate, ClassSensitivity, EdgeKey, EndpointGradient, EvaluatedClassSpace,
    LineageId, PersistenceAtlas, TopologyEvent, TopologyEventKind, UpdateMode,
};
pub use point::{
    CoordinateDerivative, PointAtlasUpdate, PointClassSensitivity, PointEndpointGradient,
    PointPersistenceAtlas,
};
