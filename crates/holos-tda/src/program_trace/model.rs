use std::fmt;

use crate::classes::IntervalGroupId;
use crate::{
    ClassContinuation, ClassCorrespondence, Diagram, Error, PersistenceProgram, ProgramArtifact,
    ProgramDecodeLimits, ProgramEvent, ProgramUpdateMode, ProgramWork, SparseDistanceMatrix,
};

/// Failure while producing, decoding, or checking a program trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramTraceError {
    message: String,
}

impl ProgramTraceError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated trace rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProgramTraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "program trace: {}", self.message)
    }
}

impl std::error::Error for ProgramTraceError {}

/// Decoder limits applied before trace collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramTraceDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted update count.
    pub max_steps: usize,
    /// Largest accepted vertices in one graph.
    pub max_vertices: usize,
    /// Largest accepted edges across all embedded graphs.
    pub max_total_edges: usize,
    /// Largest accepted event count across all steps.
    pub max_events: usize,
    /// Largest accepted continuation-record count across all steps.
    pub max_continuations: usize,
    /// Largest accepted basis-transport count across all steps.
    pub max_transports: usize,
    /// Largest accepted correspondence-record count across all steps.
    pub max_correspondences: usize,
    /// Largest accepted correspondence-vector count across all steps.
    pub max_correspondence_vectors: usize,
    /// Largest accepted correspondence-term count across all steps.
    pub max_correspondence_terms: usize,
    /// Largest accepted bars across all step diagrams.
    pub max_bars: usize,
    /// Largest accepted bytes across all program checkpoints.
    pub max_checkpoint_bytes: usize,
    /// Limits for each nested program artifact.
    pub program: ProgramDecodeLimits,
}

impl Default for ProgramTraceDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_steps: 10_000_000,
            max_vertices: 1_000_000,
            max_total_edges: 200_000_000,
            max_events: 100_000_000,
            max_continuations: 100_000_000,
            max_transports: 200_000_000,
            max_correspondences: 100_000_000,
            max_correspondence_vectors: 200_000_000,
            max_correspondence_terms: 400_000_000,
            max_bars: 100_000_000,
            max_checkpoint_bytes: 1 << 30,
            program: ProgramDecodeLimits::default(),
        }
    }
}

/// One graph update and its declared result.
#[derive(Debug, Clone)]
pub struct ProgramTraceStep {
    pub(crate) graph: SparseDistanceMatrix,
    pub(crate) mode: ProgramUpdateMode,
    pub(crate) work: ProgramWork,
    pub(crate) events: Vec<ProgramEvent>,
    pub(crate) continuation: Vec<ClassContinuation>,
    pub(crate) correspondence: Vec<ClassCorrespondence>,
    pub(crate) diagram: Diagram,
    pub(crate) checkpoint: Option<ProgramArtifact>,
}

impl ProgramTraceStep {
    /// Updated graph embedded in the trace.
    pub fn graph(&self) -> &SparseDistanceMatrix {
        &self.graph
    }

    /// Declared execution mode.
    pub fn mode(&self) -> ProgramUpdateMode {
        self.mode
    }

    /// Exact work charged by the producer.
    pub fn work(&self) -> ProgramWork {
        self.work
    }

    /// Declared topology and algebraic events.
    pub fn events(&self) -> &[ProgramEvent] {
        &self.events
    }

    /// Declared class-space continuation records.
    pub fn continuation(&self) -> &[ClassContinuation] {
        &self.continuation
    }

    /// Exact class-space relations on common filtered subcomplexes.
    pub fn correspondence(&self) -> &[ClassCorrespondence] {
        &self.correspondence
    }

    /// Exact diagram at this step.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// New full checkpoint, present only after repair or recompilation.
    pub fn checkpoint(&self) -> Option<&ProgramArtifact> {
        self.checkpoint.as_ref()
    }
}

/// Sequence of program updates with nested proofs.
#[derive(Debug, Clone)]
pub struct ProgramTraceArtifact {
    pub(crate) initial_graph: SparseDistanceMatrix,
    pub(crate) initial_program: ProgramArtifact,
    pub(crate) steps: Vec<ProgramTraceStep>,
}

/// One replayed program step.
#[derive(Debug, Clone)]
pub struct VerifiedProgramTraceStep {
    /// Execution mode.
    pub mode: ProgramUpdateMode,
    /// Work counts.
    pub work: ProgramWork,
    /// Topology and algebraic events.
    pub events: Vec<ProgramEvent>,
    /// Class-space continuation.
    pub continuation: Vec<ClassContinuation>,
    /// Exact class-space correspondence.
    pub correspondence: Vec<ClassCorrespondence>,
    /// Exact diagram.
    pub diagram: Diagram,
}

/// Result of verifying a program trace.
#[derive(Debug, Clone)]
pub struct VerifiedProgramTrace {
    /// Initial diagram and H1 class spaces.
    pub initial_result: crate::ExplainedDiagram,
    /// Update steps.
    pub steps: Vec<VerifiedProgramTraceStep>,
    /// Ready program at the final graph.
    pub final_program: PersistenceProgram,
}

#[derive(Default)]
pub(crate) struct TraceTotals {
    pub(crate) events: usize,
    pub(crate) continuations: usize,
    pub(crate) transports: usize,
    pub(crate) correspondences: usize,
    pub(crate) correspondence_vectors: usize,
    pub(crate) correspondence_terms: usize,
    pub(crate) bars: usize,
    pub(crate) checkpoint_bytes: usize,
}

pub(crate) struct TraceHeader {
    pub(crate) step_count: usize,
    pub(crate) initial_program_bytes: usize,
}

pub(crate) struct StepCounts {
    pub(crate) events: usize,
    pub(crate) continuations: usize,
    pub(crate) correspondences: usize,
    pub(crate) bars: usize,
    pub(crate) checkpoint_bytes: usize,
}

pub(crate) struct DecodedStepBody {
    pub(crate) events: Vec<ProgramEvent>,
    pub(crate) continuation: Vec<ClassContinuation>,
    pub(crate) correspondence: Vec<ClassCorrespondence>,
    pub(crate) diagram: Diagram,
}

pub(crate) struct CorrespondenceHeader {
    pub(crate) old_space: IntervalGroupId,
    pub(crate) new_space: IntervalGroupId,
    pub(crate) scale: f64,
    pub(crate) old_rank: usize,
    pub(crate) new_rank: usize,
    pub(crate) old_image_rank: usize,
    pub(crate) new_image_rank: usize,
    pub(crate) relation_rank: usize,
    pub(crate) basis_count: usize,
}

impl From<crate::ProgramUpdate> for VerifiedProgramTraceStep {
    fn from(update: crate::ProgramUpdate) -> Self {
        Self {
            mode: update.mode,
            work: update.work,
            events: update.events,
            continuation: update.continuation,
            correspondence: update.correspondence,
            diagram: update.result.diagram,
        }
    }
}

impl From<ProgramTraceError> for Error {
    fn from(error: ProgramTraceError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}
