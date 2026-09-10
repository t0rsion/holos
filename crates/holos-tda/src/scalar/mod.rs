mod execution;
mod model;
mod routing;

#[cfg(test)]
mod tests;

pub(crate) use execution::collapse_and_solve;
pub use execution::{rips_persistence, rips_persistence_sparse, rips_persistence_with_classes};
pub use model::{
    Bar, CollapseSchedule, DenseStorage, Diagram, Engine, Error, GraphFactorization, Result,
    RipsParams,
};
