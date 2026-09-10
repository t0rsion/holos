use std::fmt;

use crate::{
    EdgeKey, Error, ExplainedDiagram, IntervalGroupId, ProgramTraceArtifact,
    ProgramTraceDecodeLimits,
};

/// Deterministic search budget for a restricted intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterventionBudget {
    /// Largest number of candidate edits to check.
    pub max_candidates: usize,
}

impl InterventionBudget {
    /// Create a candidate-count budget.
    pub fn new(max_candidates: usize) -> Self {
        Self { max_candidates }
    }
}

impl Default for InterventionBudget {
    fn default() -> Self {
        Self { max_candidates: 1 }
    }
}

/// Strength of the returned intervention claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InterventionStatus {
    /// The feasible edit meets its lower bound inside the checked region.
    Optimal,
    /// The feasible edit has distinct checked lower and upper bounds.
    BoundedGap,
    /// The declared candidate budget ended without a continued feasible edit.
    BudgetLimited,
}

/// One independent edge-weight change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeWeightEdit {
    /// Edited edge.
    pub edge: EdgeKey,
    /// Weight before the edit.
    pub before: f64,
    /// Weight after the edit.
    pub after: f64,
}

/// Result of one restricted H1 intervention search.
#[derive(Debug, Clone)]
pub struct H1Intervention {
    /// Target class space at the initial graph.
    pub target: IntervalGroupId,
    /// Requested latest death scale.
    pub target_scale: f64,
    /// Strength of the returned claim.
    pub status: InterventionStatus,
    /// Lower bound on the maximum absolute edge change.
    pub lower_bound: f64,
    /// Feasible maximum absolute edge change, when present.
    pub upper_bound: Option<f64>,
    /// Feasible edge edits, empty when no candidate was certified.
    pub edits: Vec<EdgeWeightEdit>,
    /// Exact result after the edit, when present.
    pub result: Option<ExplainedDiagram>,
    /// Intervention artifact, when present.
    pub artifact: Option<InterventionArtifact>,
}
/// Failure while decoding or checking an intervention artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterventionError {
    message: String,
}

impl InterventionError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated intervention rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InterventionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "intervention artifact: {}", self.message)
    }
}

impl std::error::Error for InterventionError {}

impl From<InterventionError> for Error {
    fn from(error: InterventionError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

/// Decoder limits applied before intervention records are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct InterventionDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted edge-edit count.
    pub max_edits: usize,
    /// Largest accepted nested trace in bytes.
    pub max_trace_bytes: usize,
    /// Limits for the nested program trace.
    pub trace: ProgramTraceDecodeLimits,
}

impl Default for InterventionDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_edits: 100_000_000,
            max_trace_bytes: 1 << 30,
            trace: ProgramTraceDecodeLimits::default(),
        }
    }
}

/// Feasible H1 intervention with a nested program trace.
#[derive(Debug, Clone)]
pub struct InterventionArtifact {
    pub(super) target: IntervalGroupId,
    pub(super) target_scale: f64,
    pub(super) status: InterventionStatus,
    pub(super) lower_bound: f64,
    pub(super) upper_bound: f64,
    pub(super) edits: Vec<EdgeWeightEdit>,
    pub(super) trace: ProgramTraceArtifact,
}

impl InterventionArtifact {
    /// Target class-space identifier.
    pub fn target(&self) -> IntervalGroupId {
        self.target
    }

    /// Requested latest death scale.
    pub fn target_scale(&self) -> f64 {
        self.target_scale
    }

    /// Strength of the claim.
    pub fn status(&self) -> InterventionStatus {
        self.status
    }

    /// Lower bound on the maximum edge change.
    pub fn lower_bound(&self) -> f64 {
        self.lower_bound
    }

    /// Feasible maximum edge change.
    pub fn upper_bound(&self) -> f64 {
        self.upper_bound
    }

    /// Applied independent edge edits.
    pub fn edits(&self) -> &[EdgeWeightEdit] {
        &self.edits
    }

    /// Nested program trace.
    pub fn trace(&self) -> &ProgramTraceArtifact {
        &self.trace
    }
}

/// Result of verifying a feasible intervention.
#[derive(Debug, Clone)]
pub struct VerifiedIntervention {
    /// Strength of the claim.
    pub status: InterventionStatus,
    /// Initial target class space.
    pub target: IntervalGroupId,
    /// Requested latest death scale.
    pub target_scale: f64,
    /// Lower bound on the maximum edge change.
    pub lower_bound: f64,
    /// Feasible upper bound.
    pub upper_bound: f64,
    /// Edge edits.
    pub edits: Vec<EdgeWeightEdit>,
    /// Exact final diagram and class spaces.
    pub result: ExplainedDiagram,
}
