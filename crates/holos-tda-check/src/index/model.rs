use std::collections::BTreeMap;

use crate::{Graph, ProofBar, ProofColumn};

/// Counts from a checked complete index snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedIndexSnapshot {
    /// Checked root content identifier.
    pub root: [u8; 32],
    /// Interface nodes checked from algebraic reductions.
    pub nodes_checked: usize,
    /// Interface nodes checked by separator composition.
    pub composed_nodes_checked: usize,
    /// Interface nodes checked as relative filtered cores.
    pub relative_nodes_checked: usize,
    /// Edge-boundary columns checked.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked.
    pub triangle_columns_checked: usize,
    /// Boundary columns checked above the triangle dimension.
    pub higher_columns_checked: usize,
}

/// Counts from one checked warm index delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedIndexDelta {
    /// Root required before the delta was applied.
    pub old_root: [u8; 32],
    /// Root established by the delta.
    pub new_root: [u8; 32],
    /// Edge values changed by the delta.
    pub edge_changes: usize,
    /// New interface nodes checked from algebraic reductions.
    pub nodes_checked: usize,
    /// New interface nodes checked by separator composition.
    pub composed_nodes_checked: usize,
    /// New interface nodes checked as relative filtered cores.
    pub relative_nodes_checked: usize,
    /// References from new nodes to already checked child nodes.
    pub reused_child_references: usize,
    /// Edge-boundary columns checked.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked.
    pub triangle_columns_checked: usize,
    /// Boundary columns checked above the triangle dimension.
    pub higher_columns_checked: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct InterfaceProof {
    pub(super) digest: [u8; 32],
    pub(super) vertices: Vec<usize>,
    pub(super) edge_positions: Vec<usize>,
    pub(super) separator: Vec<usize>,
    pub(super) protected_vertices: Vec<usize>,
    pub(super) children: Vec<[u8; 32]>,
    pub(super) mode: InterfaceMode,
    pub(super) graded_columns: Vec<Vec<ProofColumn>>,
    pub(super) relative_artifact: Vec<u8>,
    pub(super) diagram: Vec<ProofBar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InterfaceMode {
    Relative,
    Materialized,
    Disjoint,
    ZeroSimplex,
    ZeroCone,
}

#[derive(Debug)]
pub(super) struct Snapshot {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) threshold: Option<f64>,
    pub(super) graph: Graph,
    pub(super) root: [u8; 32],
    pub(super) nodes: Vec<InterfaceProof>,
    pub(super) diagram: Vec<ProofBar>,
}

#[derive(Debug)]
pub(super) struct Delta {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) threshold: Option<f64>,
    pub(super) vertex_count: usize,
    pub(super) edge_count: usize,
    pub(super) old_root: [u8; 32],
    pub(super) new_root: [u8; 32],
    pub(super) edge_changes: Vec<(usize, f64)>,
    pub(super) nodes: Vec<InterfaceProof>,
    pub(super) diagram: Vec<ProofBar>,
}

/// Stateful checker for one versioned persistence-index envelope.
///
/// Construct it from a complete snapshot. Each accepted delta advances the
/// verified root and retains old interface nodes.
#[derive(Debug, Clone)]
pub struct IndexProofState {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) threshold: Option<f64>,
    pub(super) graph: Graph,
    pub(super) nodes: BTreeMap<[u8; 32], InterfaceProof>,
    pub(super) root: [u8; 32],
    pub(super) diagram: Vec<ProofBar>,
}
