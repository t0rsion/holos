use holos_tda::Diagram;
use holos_tda::collapse::CollapsedRips;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    V2,
    V1,
    V1Ordered,
    /// The shipped pipeline: one run-wide pool for the ordered collapse and
    /// the reduction, through `rips_persistence` with `collapse_edges` and
    /// the ordered schedule selected.
    V1Product,
    NoCollapse,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InputKind {
    Sparse,
    Dense,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    V2,
    V1,
    V1Ordered,
    V1Product,
    NoCollapse,
    /// The rounds study set: v2-cN, v1-c1, none.
    Rounds,
    All,
}

pub(super) struct Args {
    pub(super) input: String,
    pub(super) entry: String,
    pub(super) threshold: f64,
    pub(super) threshold_text: String,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) collapse_threads: Vec<usize>,
    pub(super) reducer_threads: usize,
    pub(super) reps: usize,
    pub(super) mode: Mode,
    pub(super) collapse_input: InputKind,
}

pub(super) struct ArgsBuilder {
    pub(super) input: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) threshold_text: Option<String>,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) collapse_threads: Vec<usize>,
    pub(super) reducer_threads: usize,
    pub(super) reps: usize,
    pub(super) mode: Mode,
    pub(super) collapse_input: InputKind,
}

pub(super) struct Config {
    pub(super) name: String,
    pub(super) kind: Kind,
    pub(super) collapse_threads: usize,
}

/// One pipeline run: the phase clocks plus what the run produced.
pub(super) struct Outcome {
    pub(super) phases: Vec<(&'static str, f64)>,
    pub(super) diagram: Diagram,
    pub(super) counters: Option<Counters>,
    pub(super) graph_edges: Option<usize>,
    /// The collapse result itself, kept only by the agreement runs that
    /// the ordered gate compares. A timed repetition drops it.
    pub(super) collapsed: Option<CollapsedRips>,
}

/// Collapse counters and clocks of one run, from CollapseStats,
/// CollapseTimings, and the certificate.
pub(super) struct Counters {
    pub(super) algorithm_version: u32,
    pub(super) terminal_level: f64,
    pub(super) input_edges: usize,
    pub(super) output_edges: usize,
    pub(super) removed_edges: usize,
    pub(super) epochs: usize,
    pub(super) edge_tests: usize,
    pub(super) logical_tests: usize,
    pub(super) invalidated_results: usize,
    pub(super) global_invalidations: usize,
    pub(super) window_batches: usize,
    pub(super) window_slots_offered: usize,
    pub(super) window_members_formed: usize,
    pub(super) window_members_reused: usize,
    pub(super) witness_segments: usize,
    pub(super) max_common_neighborhood: usize,
    /// Nanoseconds in the parallel test phase. Zero where the path does not
    /// clock its subphases.
    pub(super) predicate_ns: u64,
    /// Nanoseconds in the serial retirement walk, repairs included.
    pub(super) retirement_ns: u64,
    /// Nanoseconds re-evaluating invalidated verdicts.
    pub(super) repair_ns: u64,
    /// Removals per epoch, as (epoch, width), for the epochs that removed
    /// something.
    pub(super) batch_widths: Vec<(usize, usize)>,
}

pub(super) struct Summary {
    pub(super) median: f64,
    pub(super) iqr: f64,
    pub(super) q1: f64,
    pub(super) q3: f64,
    pub(super) min: f64,
    pub(super) max: f64,
}

pub(super) struct Verification {
    pub(super) outcomes: Vec<Outcome>,
    pub(super) gate_lines: Vec<String>,
}

pub(super) struct Samples {
    pub(super) phases: Vec<Vec<Vec<f64>>>,
    pub(super) counters: Vec<Vec<Counters>>,
}

pub(super) struct OrderedGate {
    pub(super) serial_v1: Option<CollapsedRips>,
    pub(super) lines: Vec<String>,
}

impl Default for ArgsBuilder {
    fn default() -> Self {
        Self {
            input: None,
            entry: None,
            threshold_text: None,
            max_dim: 1,
            modulus: 2,
            collapse_threads: vec![1],
            reducer_threads: 1,
            reps: 5,
            mode: Mode::All,
            collapse_input: InputKind::Sparse,
        }
    }
}
