use crate::proof::{ProofBar, ProofColumn};

#[derive(Debug, Clone)]
pub(super) struct ProgramClaim {
    pub(super) modulus: u32,
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) input_digest: [u8; 32],
    pub(super) diagram: Vec<ProofBar>,
    pub(super) atoms: Vec<ProgramAtomClaim>,
}

#[derive(Debug, Clone)]
pub(super) struct ProgramAtomClaim {
    pub(super) id: usize,
    pub(super) vertices: Vec<usize>,
    pub(super) edges: Vec<[usize; 2]>,
    pub(super) atlas: AtlasClaim,
}

#[derive(Debug, Clone)]
pub(super) struct AtlasClaim {
    pub(super) modulus: u32,
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) input_digest: [u8; 32],
    pub(super) diagram: Vec<ProofBar>,
    pub(super) spaces: Vec<SpaceClaim>,
    pub(super) reduction: ReductionClaim,
}

#[derive(Debug, Clone)]
pub(super) struct SpaceClaim {
    pub(super) id: [u8; 32],
    pub(super) interval: ProofBar,
    pub(super) critical_pairs: Vec<CriticalPairClaim>,
    pub(super) basis: Vec<ClassClaim>,
}

#[derive(Debug, Clone)]
pub(super) struct ClassClaim {
    pub(super) id: [u8; 32],
    pub(super) basis_index: usize,
    pub(super) scale: f64,
    pub(super) terms: Vec<CocycleTermClaim>,
    pub(super) provenance: Option<ProvenanceClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ProvenanceClaim {
    pub(super) source_graph_digest: [u8; 32],
    pub(super) class_digest: [u8; 32],
    pub(super) interval: ProofBar,
    pub(super) modulus: u32,
    pub(super) scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CocycleTermClaim {
    pub(super) u: usize,
    pub(super) v: usize,
    pub(super) coefficient: u32,
}

#[derive(Debug, Clone)]
pub(super) struct CriticalPairClaim {
    pub(super) birth: SimplexClaim,
    pub(super) death: Option<SimplexClaim>,
}

#[derive(Debug, Clone)]
pub(super) struct SimplexClaim {
    pub(super) vertices: Vec<usize>,
    pub(super) value: f64,
}

#[derive(Debug, Clone)]
pub(super) struct ReductionClaim {
    pub(super) modulus: u32,
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) graph_digest: [u8; 32],
    pub(super) edge_columns: Vec<ProofColumn>,
    pub(super) triangle_columns: Vec<ProofColumn>,
    pub(super) diagram: Vec<ProofBar>,
}
