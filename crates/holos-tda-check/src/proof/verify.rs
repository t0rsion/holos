use std::collections::{BTreeMap, BTreeSet};

use crate::{MODULUS_LIMIT, is_prime};

use super::graph::{
    Block, Graph, canonicalize_diagram, check_reduction, diagrams_equal, h0_diagram, program_blocks,
};
use super::model::{AtomProof, ProofBar, ProofBundle, ProofError, SnapshotProof};

pub(crate) fn check_node_scope(vertices: &[usize], edges: &[[usize; 2]]) -> Result<(), ProofError> {
    if vertices.len() < 3
        || vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || edges.len() < vertices.len()
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        Err(ProofError::new("reduction node scope is not canonical"))
    } else {
        Ok(())
    }
}

pub(crate) fn check_node_edges(vertices: &[usize], edges: &[[usize; 2]]) -> Result<(), ProofError> {
    if edges.iter().any(|&[u, v]| {
        u >= v || vertices.binary_search(&u).is_err() || vertices.binary_search(&v).is_err()
    }) {
        Err(ProofError::new("reduction node has an invalid edge"))
    } else {
        Ok(())
    }
}

pub(crate) fn check_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn check_bundle_nodes(
    nodes: &[AtomProof],
    modulus: u32,
) -> Result<BTreeSet<[u8; 32]>, ProofError> {
    let mut digests = BTreeSet::new();
    for node in nodes {
        node.check_shape(modulus)?;
        if node.compute_digest() != node.digest || !digests.insert(node.digest) {
            return Err(ProofError::new(
                "reduction-node digest is wrong or duplicated",
            ));
        }
    }
    Ok(digests)
}

pub(crate) fn check_bundle_snapshots(
    snapshots: &[SnapshotProof],
    digests: &BTreeSet<[u8; 32]>,
) -> Result<(), ProofError> {
    for snapshot in snapshots {
        snapshot.check_shape()?;
        if snapshot
            .atom_refs
            .iter()
            .any(|digest| !digests.contains(digest))
        {
            return Err(ProofError::new(
                "snapshot references an unknown reduction node",
            ));
        }
    }
    Ok(())
}

/// Counts from a checked `HOLOSPF` proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProof {
    /// Checked snapshot count.
    pub snapshots: usize,
    /// Unique checked reduction-node count.
    pub unique_nodes: usize,
    /// Snapshot references already present in the previous snapshot.
    pub reused_references: usize,
    /// Node references whose identical weighted reduction was already checked.
    pub cached_references: usize,
    /// Edge-boundary columns checked across all node references.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked across all node references.
    pub triangle_columns_checked: usize,
}

impl ProofBundle {
    /// Check every snapshot against reconstructed H0 and H1 diagrams.
    ///
    /// The checker reconstructs filtered edge and triangle boundaries.
    /// It checks every declared `D V = R` factorization and the cyclic-atom
    /// decomposition.
    pub fn verify(&self) -> Result<VerifiedProof, ProofError> {
        self.check_shape()?;
        let mut verifier = BundleVerifier::new(self);
        for (snapshot_index, snapshot) in self.snapshots.iter().enumerate() {
            verifier.verify_snapshot(snapshot_index, snapshot)?;
        }
        Ok(verifier.summary())
    }
}

type ReductionCache = BTreeMap<([u8; 32], Vec<u64>), Vec<ProofBar>>;

struct BundleVerifier<'a> {
    bundle: &'a ProofBundle,
    nodes: BTreeMap<[u8; 32], &'a AtomProof>,
    previous: BTreeSet<[u8; 32]>,
    reductions: ReductionCache,
    reused_references: usize,
    cached_references: usize,
    edge_columns_checked: usize,
    triangle_columns_checked: usize,
}

impl<'a> BundleVerifier<'a> {
    fn new(bundle: &'a ProofBundle) -> Self {
        Self {
            bundle,
            nodes: bundle
                .nodes
                .iter()
                .map(|node| (node.digest, node))
                .collect(),
            previous: BTreeSet::new(),
            reductions: BTreeMap::new(),
            reused_references: 0,
            cached_references: 0,
            edge_columns_checked: 0,
            triangle_columns_checked: 0,
        }
    }

    fn verify_snapshot(
        &mut self,
        snapshot_index: usize,
        snapshot: &SnapshotProof,
    ) -> Result<(), ProofError> {
        let graph = Graph::new(snapshot.vertex_count, &snapshot.edges)?;
        let cyclic = program_blocks(&graph, self.bundle.threshold)?
            .into_iter()
            .filter(|block| block.edges.len() >= block.vertices.len())
            .collect::<Vec<_>>();
        verify_atom_count(snapshot_index, snapshot, cyclic.len())?;
        let mut diagram = h0_diagram(&graph, self.bundle.threshold)?;
        for (position, (block, digest)) in cyclic.iter().zip(&snapshot.atom_refs).enumerate() {
            diagram.extend(self.verify_atom(snapshot_index, position, &graph, block, digest)?);
        }
        canonicalize_diagram(&mut diagram);
        verify_snapshot_diagram(snapshot_index, &diagram, &snapshot.diagram)?;
        self.record_reuse(&snapshot.atom_refs);
        Ok(())
    }

    fn verify_atom(
        &mut self,
        snapshot: usize,
        position: usize,
        graph: &Graph,
        block: &Block,
        digest: &[u8; 32],
    ) -> Result<Vec<ProofBar>, ProofError> {
        let node = self.nodes[digest];
        if node.vertices != block.vertices || node.edges != block.edges {
            return Err(ProofError::new(format!(
                "snapshot {snapshot} atom {position} differs from the checked decomposition"
            )));
        }
        let local = graph.local(&node.vertices, &node.edges)?;
        self.h1_for_node(node, &local)
    }

    fn h1_for_node(
        &mut self,
        node: &AtomProof,
        local: &Graph,
    ) -> Result<Vec<ProofBar>, ProofError> {
        let key = (
            node.digest,
            local
                .edges
                .iter()
                .map(|edge| edge.value.to_bits())
                .collect(),
        );
        if let Some(checked) = self.reductions.get(&key) {
            self.cached_references += 1;
            return Ok(checked.clone());
        }
        let checked = check_reduction(
            local,
            self.bundle.threshold,
            self.bundle.modulus,
            &node.edge_columns,
            &node.triangle_columns,
        )?;
        self.edge_columns_checked += node.edge_columns.len();
        self.triangle_columns_checked += node.triangle_columns.len();
        let h1 = checked
            .diagram
            .into_iter()
            .filter(|bar| bar.dimension == 1)
            .collect::<Vec<_>>();
        self.reductions.insert(key, h1.clone());
        Ok(h1)
    }

    fn record_reuse(&mut self, references: &[[u8; 32]]) {
        let current = references.iter().copied().collect::<BTreeSet<_>>();
        self.reused_references += current.intersection(&self.previous).count();
        self.previous = current;
    }

    fn summary(&self) -> VerifiedProof {
        VerifiedProof {
            snapshots: self.bundle.snapshots.len(),
            unique_nodes: self.bundle.nodes.len(),
            reused_references: self.reused_references,
            cached_references: self.cached_references,
            edge_columns_checked: self.edge_columns_checked,
            triangle_columns_checked: self.triangle_columns_checked,
        }
    }
}

fn verify_atom_count(
    snapshot_index: usize,
    snapshot: &SnapshotProof,
    actual: usize,
) -> Result<(), ProofError> {
    if actual != snapshot.atom_refs.len() {
        Err(ProofError::new(format!(
            "snapshot {snapshot_index} has {actual} cyclic atoms but {} references",
            snapshot.atom_refs.len()
        )))
    } else {
        Ok(())
    }
}

fn verify_snapshot_diagram(
    snapshot_index: usize,
    checked: &[ProofBar],
    claimed: &[ProofBar],
) -> Result<(), ProofError> {
    if diagrams_equal(checked, claimed) {
        Ok(())
    } else {
        Err(ProofError::new(format!(
            "snapshot {snapshot_index} diagram differs from the checked composition"
        )))
    }
}
