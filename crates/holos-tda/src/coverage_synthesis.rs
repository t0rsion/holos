//! Failure-tolerant specifications for exact relative coverage.
//!
//! A finite state lists its communication edges and the sensors active before
//! a plan. An action activates one additional non-fence sensor in declared
//! states. A plan is feasible only if the controlled-boundary criterion holds
//! after every maximal allowed sensor failure in every state.

mod artifact;
mod evaluate;
mod model;
mod search;
mod wire;

#[cfg(all(test, holos_repository_tests))]
mod tests;

pub use evaluate::evaluate_coverage_plan;
pub(crate) use evaluate::evaluate_coverage_plan_states_prevalidated;
pub use model::{
    CoverageAction, CoverageComponent, CoverageCounterexample, CoveragePlanEvaluation,
    CoverageSource, CoverageSpecification, CoverageState, CoverageSynthesisArtifact,
    CoverageSynthesisLimits, CoverageSynthesisStatus,
};
