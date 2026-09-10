use crate::proof::{Graph, ProofBar};

use super::claim::{CocycleTermClaim, ProgramClaim, ReductionClaim};
use super::graph::ProgramGraph;
use super::model::ProgramProofLimits;
use super::trace_region::ReuseRegion;

/// Decoder and verifier limits for one `HOLOSDLT` trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramTraceProofLimits {
    /// Largest accepted trace envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted update count.
    pub max_steps: usize,
    /// Largest accepted vertices in one embedded graph.
    pub max_vertices: usize,
    /// Largest accepted total embedded graph edges.
    pub max_total_edges: usize,
    /// Largest accepted event count across all steps.
    pub max_events: usize,
    /// Largest accepted continuation count across all steps.
    pub max_continuations: usize,
    /// Largest accepted basis transport count across all steps.
    pub max_transports: usize,
    /// Largest accepted correspondence count across all steps.
    pub max_correspondences: usize,
    /// Largest accepted correspondence vector count across all steps.
    pub max_correspondence_vectors: usize,
    /// Largest accepted correspondence term count across all steps.
    pub max_correspondence_terms: usize,
    /// Largest accepted dense relation matrix size in field cells.
    pub max_relation_cells: usize,
    /// Largest accepted step-bar count across all steps.
    pub max_bars: usize,
    /// Largest accepted bytes across all nested checkpoints.
    pub max_checkpoint_bytes: usize,
    /// Limits applied to every nested `HOLOSPRG` artifact.
    pub program: ProgramProofLimits,
}

impl Default for ProgramTraceProofLimits {
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
            max_relation_cells: 10_000_000,
            max_bars: 100_000_000,
            max_checkpoint_bytes: 1 << 30,
            program: ProgramProofLimits::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TraceLimits {
    pub(super) max_bytes: usize,
    pub(super) max_steps: usize,
    pub(super) max_vertices: usize,
    pub(super) max_total_edges: usize,
    pub(super) max_events: usize,
    pub(super) max_continuations: usize,
    pub(super) max_transports: usize,
    pub(super) max_correspondences: usize,
    pub(super) max_correspondence_vectors: usize,
    pub(super) max_correspondence_terms: usize,
    pub(super) max_bars: usize,
    pub(super) max_checkpoint_bytes: usize,
}

impl Default for TraceLimits {
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
        }
    }
}

impl From<ProgramTraceProofLimits> for TraceLimits {
    fn from(limits: ProgramTraceProofLimits) -> Self {
        Self {
            max_bytes: limits.max_bytes,
            max_steps: limits.max_steps,
            max_vertices: limits.max_vertices,
            max_total_edges: limits.max_total_edges,
            max_events: limits.max_events,
            max_continuations: limits.max_continuations,
            max_transports: limits.max_transports,
            max_correspondences: limits.max_correspondences,
            max_correspondence_vectors: limits.max_correspondence_vectors,
            max_correspondence_terms: limits.max_correspondence_terms,
            max_bars: limits.max_bars,
            max_checkpoint_bytes: limits.max_checkpoint_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceMode {
    Reused,
    Repaired,
    Recompiled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceEventKind {
    VertexSetChanged,
    EdgeSetChanged,
    ThresholdCrossing,
    GuardFailed,
    AtomRebuilt,
    ReductionSuffixRepaired,
    SeparatorContractChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceGuardKind {
    ChangeOfBasis,
    Pivot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TraceWork {
    pub(super) edges_checked: usize,
    pub(super) h0_edges_scanned: usize,
    pub(super) guards_checked: usize,
    pub(super) atoms_touched: usize,
    pub(super) atoms_reused: usize,
    pub(super) atoms_repaired: usize,
    pub(super) atoms_rebuilt: usize,
    pub(super) reduction_columns_reused: usize,
    pub(super) reduction_columns_reduced: usize,
    pub(super) reduction_column_additions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TraceEvent {
    pub(super) kind: TraceEventKind,
    pub(super) atom: Option<usize>,
    pub(super) edge: Option<[usize; 2]>,
    pub(super) guard: Option<TraceGuardKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceContinuationKind {
    Isomorphism,
    Split,
    Merge,
    Mixing,
    Birth,
    Death,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TraceContinuation {
    pub(super) kind: TraceContinuationKind,
    pub(super) old_spaces: Vec<[u8; 32]>,
    pub(super) new_spaces: Vec<[u8; 32]>,
    pub(super) transport: Vec<TraceTransport>,
}

pub(super) type TraceTransport = ([u8; 32], [u8; 32], u32);
pub(super) type TraceBasisTerm = ([u8; 32], u32);
pub(super) type TraceCorrespondenceVector = (Vec<TraceBasisTerm>, Vec<TraceBasisTerm>);

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TraceCorrespondence {
    pub(super) old_space: [u8; 32],
    pub(super) new_space: [u8; 32],
    pub(super) scale: f64,
    pub(super) old_rank: usize,
    pub(super) new_rank: usize,
    pub(super) old_image_rank: usize,
    pub(super) new_image_rank: usize,
    pub(super) relation_rank: usize,
    pub(super) basis: Vec<TraceCorrespondenceVector>,
}

#[derive(Debug, Clone)]
pub(super) struct TraceStep {
    pub(super) graph: ProgramGraph,
    pub(super) mode: TraceMode,
    pub(super) work: TraceWork,
    pub(super) events: Vec<TraceEvent>,
    pub(super) continuation: Vec<TraceContinuation>,
    pub(super) correspondence: Vec<TraceCorrespondence>,
    pub(super) diagram: Vec<ProofBar>,
    pub(super) checkpoint: Option<ProgramClaim>,
}

/// Summary of an independently checked `HOLOSDLT` trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProgramTrace {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Number of vertices in the initial graph.
    pub vertices: usize,
    /// Number of listed edges in the initial graph.
    pub edges: usize,
    /// Number of replayed update steps.
    pub steps: usize,
    /// Number of reused steps.
    pub reused_steps: usize,
    /// Number of repaired steps.
    pub repaired_steps: usize,
    /// Number of recompiled steps.
    pub recompiled_steps: usize,
    /// Number of bars in the final diagram.
    pub bars: usize,
}

#[derive(Debug, Clone)]
pub(super) struct AtomState {
    pub(super) id: usize,
    pub(super) vertices: Vec<usize>,
    pub(super) edges: Vec<[usize; 2]>,
    pub(super) graph: Graph,
    pub(super) reduction: ReductionClaim,
    pub(super) spaces: Vec<AtomSpace>,
    pub(super) region: ReuseRegion,
}

#[derive(Debug, Clone)]
pub(super) struct AtomSpace {
    pub(super) interval: ProofBar,
    pub(super) critical_pairs: Vec<LocalPair>,
    pub(super) basis: Vec<Vec<CocycleTermClaim>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalPair {
    pub(super) birth: Vec<usize>,
    pub(super) death: Option<Vec<usize>>,
}

#[derive(Debug, Clone)]
pub(super) struct ProgramState {
    pub(super) graph: ProgramGraph,
    pub(super) modulus: u32,
    pub(super) threshold: Option<f64>,
    pub(super) separator_edges: Vec<[usize; 2]>,
    pub(super) atoms: Vec<AtomState>,
    pub(super) result: ResultState,
}

#[derive(Debug, Clone)]
pub(super) struct ResultState {
    pub(super) diagram: Vec<ProofBar>,
    pub(super) spaces: Vec<ResultSpace>,
}

#[derive(Debug, Clone)]
pub(super) struct ResultSpace {
    pub(super) id: [u8; 32],
    pub(super) interval: ProofBar,
    pub(super) scale: f64,
    pub(super) basis: Vec<ResultClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResultClass {
    pub(super) id: [u8; 32],
    pub(super) terms: Vec<CocycleTermClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GlobalPair {
    pub(super) birth: Vec<usize>,
    pub(super) death: Option<Vec<usize>>,
}
