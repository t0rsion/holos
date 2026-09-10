use holos_tda::{DenseStorage, Diagram};

/// Which engine a configuration runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Engine {
    Auto,
    Dense,
    Sparse,
}

/// How the input file spells its distances.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Points,
    LowerDistance,
    Sparse,
}

pub(super) struct Args {
    pub(super) input: String,
    pub(super) entry: String,
    pub(super) threshold: f64,
    pub(super) threshold_text: String,
    pub(super) format: Format,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) reps: usize,
    pub(super) threads: usize,
    pub(super) parse_threads: usize,
    pub(super) mode: Vec<Engine>,
    pub(super) dense_storage: DenseStorage,
    pub(super) diagram_out: Option<String>,
    pub(super) emit_collapsed: Option<String>,
}

pub(super) struct ArgsBuilder {
    pub(super) input: Option<String>,
    pub(super) entry: Option<String>,
    pub(super) threshold_text: Option<String>,
    pub(super) format: Format,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) reps: usize,
    pub(super) threads: usize,
    pub(super) parse_threads: usize,
    pub(super) mode: Vec<Engine>,
    pub(super) dense_storage: DenseStorage,
    pub(super) diagram_out: Option<String>,
    pub(super) emit_collapsed: Option<String>,
}

pub(super) struct Config {
    pub(super) name: &'static str,
    pub(super) engine: Engine,
}

/// One pipeline run: the phase clocks plus what the run produced.
pub(super) struct Outcome {
    pub(super) phases: Vec<(&'static str, f64)>,
    pub(super) diagram: Diagram,
    pub(super) points: usize,
    /// Edges at or below the threshold. The sparse configuration counts
    /// them while it builds its graph; the dense one never forms them.
    pub(super) graph_edges: Option<usize>,
}

pub(super) struct Summary {
    pub(super) median: f64,
    pub(super) iqr: f64,
    pub(super) q1: f64,
    pub(super) q3: f64,
    pub(super) min: f64,
    pub(super) max: f64,
}

/// The parsed input, before any matrix exists.
pub(super) enum Parsed {
    Points(Vec<Vec<f64>>),
    Condensed(Vec<f64>),
    /// Vertex count and `(i, j, d)` triplets of a sparse input. The vertex
    /// count is one more than the largest index in the file, the rule
    /// holos and ripser both follow.
    Triplets(usize, Vec<(usize, usize, f64)>),
}
