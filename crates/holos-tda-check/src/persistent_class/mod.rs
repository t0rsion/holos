//! Independent verification of one `HOLOSPC` persistent H1 class artifact.

mod model;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    PersistenceCycleTerm, PersistenceTriangleTerm, PersistentCocycleTerm, PersistentCriticalPair,
    PersistentSourceEdge, VerifiedPersistentClass,
};
pub use verify::{is_persistent_class, verify_persistent_class};
