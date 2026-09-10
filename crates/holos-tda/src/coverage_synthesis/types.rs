//! Coverage actions, evaluations, and status values.

use std::fmt;

use crate::{Error, Result};

use super::specification::CoverageSpecification;

/// One candidate sensor activation and the states where it is available.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverageAction {
    /// Sensor vertex activated by this action.
    pub vertex: usize,
    /// Positive additive action cost.
    pub cost: u64,
    pub(crate) states: Vec<usize>,
}

impl CoverageAction {
    /// Construct an activation and canonicalize its affected states.
    pub fn new(vertex: usize, cost: u64, mut states: Vec<usize>) -> Self {
        states.sort_unstable();
        states.dedup();
        Self {
            vertex,
            cost,
            states,
        }
    }

    /// State indices where this sensor is activated.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Construct an activation that applies to every state.
    pub fn throughout(vertex: usize, cost: u64, specification: &CoverageSpecification) -> Self {
        Self::new(vertex, cost, (0..specification.states.len()).collect())
    }
}

/// One independent state-action incidence component.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageComponent {
    pub(crate) states: Vec<usize>,
    pub(crate) actions: Vec<usize>,
}

impl CoverageComponent {
    /// State indices in this component.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Action indices in this component.
    pub fn actions(&self) -> &[usize] {
        &self.actions
    }
}

/// First state and failure set that refutes a selected plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageCounterexample {
    /// State index in the canonical specification.
    pub state: usize,
    /// Failed sensor vertices in ascending order.
    pub failed_vertices: Vec<usize>,
}

/// Exact result of evaluating one selected activation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveragePlanEvaluation {
    /// True when every state survives every allowed failure.
    pub criterion_holds: bool,
    /// Number of state and maximal-failure pairs checked.
    pub checks: usize,
    /// Smallest witness support among accepted checks.
    pub minimum_witness_triangles: Option<usize>,
    /// First canonical failed check, when one exists.
    pub counterexample: Option<CoverageCounterexample>,
}

/// Completeness status of a coverage synthesis result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageSynthesisStatus {
    /// The selected activations have minimum total cost.
    Optimal,
    /// No plan within the activation limit satisfies the specification.
    Infeasible,
    /// A producer work limit stopped search before a complete proof.
    SearchIncomplete,
}

impl CoverageSynthesisStatus {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Optimal => 1,
            Self::Infeasible => 2,
            Self::SearchIncomplete => 3,
        }
    }

    pub(crate) fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(Error::InvalidInput(
                "coverage synthesis status is invalid".into(),
            )),
        }
    }
}

impl fmt::Display for CoverageSynthesisStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationClaim {
    pub(crate) criterion_holds: bool,
    pub(crate) checks: usize,
    pub(crate) minimum_witness_triangles: Option<usize>,
}

impl From<&CoveragePlanEvaluation> for EvaluationClaim {
    fn from(evaluation: &CoveragePlanEvaluation) -> Self {
        Self {
            criterion_holds: evaluation.criterion_holds,
            checks: evaluation.checks,
            minimum_witness_triangles: evaluation.minimum_witness_triangles,
        }
    }
}
