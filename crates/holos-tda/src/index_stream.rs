//! Stateful index streams with proof output.

use crate::{
    CorrespondenceMode, Error, IndexDeltaProof, IndexProofError, IndexSnapshotProof,
    IndexTransition, PersistenceIndex, Result, SparseDistanceMatrix, TopologyPatch,
};

/// Proof record emitted by one stream step.
#[derive(Debug, Clone)]
pub enum IndexStreamProof {
    /// Warm proof against the preceding root.
    Delta(IndexDeltaProof),
    /// Cold proof after the listed-edge envelope changed.
    Snapshot(IndexSnapshotProof),
}

impl IndexStreamProof {
    /// Encode this record in its canonical format.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, IndexProofError> {
        match self {
            Self::Delta(proof) => proof.encode(),
            Self::Snapshot(proof) => proof.encode(),
        }
    }

    /// Whether this record starts a new proof state.
    pub fn is_snapshot(&self) -> bool {
        matches!(self, Self::Snapshot(_))
    }
}

/// One committed step in an [`IndexStream`].
#[derive(Debug, Clone)]
pub struct IndexStreamStep {
    /// One-based sequence number in this stream.
    pub sequence: u64,
    /// Exact index transition.
    pub transition: IndexTransition,
    /// Proof of the new root.
    pub proof: IndexStreamProof,
}

/// Stateful transaction stream over persistence-index versions.
#[derive(Debug, Clone)]
pub struct IndexStream {
    index: PersistenceIndex,
    sequence: u64,
}

impl IndexStream {
    /// Start a stream at a compiled index version.
    pub fn new(index: PersistenceIndex) -> Self {
        Self { index, sequence: 0 }
    }

    /// Current index version.
    pub fn current(&self) -> &PersistenceIndex {
        &self.index
    }

    /// Number of committed steps.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Produce a cold proof for the current version.
    pub fn checkpoint(&self) -> std::result::Result<IndexSnapshotProof, IndexProofError> {
        IndexSnapshotProof::from_index(&self.index)
    }

    /// Apply one active-topology patch atomically.
    pub fn apply_patch(
        &mut self,
        patch: &TopologyPatch,
        correspondence_mode: CorrespondenceMode,
    ) -> Result<IndexStreamStep> {
        let old = self.index.clone();
        let transition = old.transition_patch_with(patch, correspondence_mode)?;
        self.commit(old, transition)
    }

    /// Apply one graph state atomically.
    ///
    /// An envelope change emits a cold checkpoint. A fixed-envelope change
    /// emits a warm delta against the preceding root.
    pub fn apply_graph(
        &mut self,
        graph: &SparseDistanceMatrix,
        correspondence_mode: CorrespondenceMode,
    ) -> Result<IndexStreamStep> {
        let old = self.index.clone();
        let transition = old.transition_with(graph, correspondence_mode)?;
        self.commit(old, transition)
    }

    /// Apply an ordered patch batch atomically.
    ///
    /// If one patch fails, neither the index nor the sequence changes.
    pub fn apply_patches(
        &mut self,
        patches: &[TopologyPatch],
        correspondence_mode: CorrespondenceMode,
    ) -> Result<Vec<IndexStreamStep>> {
        let mut candidate = self.clone();
        let mut steps = Vec::with_capacity(patches.len());
        for patch in patches {
            steps.push(candidate.apply_patch(patch, correspondence_mode)?);
        }
        *self = candidate;
        Ok(steps)
    }

    fn commit(
        &mut self,
        old: PersistenceIndex,
        transition: IndexTransition,
    ) -> Result<IndexStreamStep> {
        let proof = if old.topology() == transition.index.topology()
            && old.graph().len() == transition.index.graph().len()
        {
            IndexStreamProof::Delta(
                IndexDeltaProof::between(&old, &transition.index).map_err(proof_error)?,
            )
        } else {
            IndexStreamProof::Snapshot(
                IndexSnapshotProof::from_index(&transition.index).map_err(proof_error)?,
            )
        };
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("index stream sequence overflow".into()))?;
        self.index = transition.index.clone();
        self.sequence = sequence;
        Ok(IndexStreamStep {
            sequence,
            transition,
            proof,
        })
    }
}

fn proof_error(error: IndexProofError) -> Error {
    Error::InvalidInput(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CertificateLimits, IndexEdit, IndexParams, RipsParams};

    fn graph(weight: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, weight),
                (0, 2, 1.0),
                (1, 2, 1.5),
                (0, 3, 1.1),
                (1, 3, 1.6),
                (0, 4, 1.2),
                (1, 4, 1.7),
            ],
        )
        .unwrap()
    }

    #[test]
    fn stream_emits_warm_and_cold_records() {
        let mut params = RipsParams::new(1).with_modulus(3);
        params.threshold = Some(2.0);
        let index = PersistenceIndex::compile(
            &graph(0.0),
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        let mut stream = IndexStream::new(index);
        let patch = TopologyPatch::new(vec![IndexEdit::deactivate(0, 1)]);
        let warm = stream
            .apply_patch(&patch, CorrespondenceMode::Omit)
            .unwrap();
        assert_eq!(warm.sequence, 1);
        assert!(!warm.proof.is_snapshot());

        let changed_envelope = SparseDistanceMatrix::from_triplets(5, &[(0, 1, 0.5)]).unwrap();
        let cold = stream
            .apply_graph(&changed_envelope, CorrespondenceMode::Omit)
            .unwrap();
        assert_eq!(cold.sequence, 2);
        assert!(cold.proof.is_snapshot());
    }

    #[test]
    fn patch_batch_is_atomic() {
        let mut params = RipsParams::new(1);
        params.threshold = Some(2.0);
        let index = PersistenceIndex::compile(
            &graph(0.0),
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        let mut stream = IndexStream::new(index);
        let old_root = stream.current().version();
        let patches = [
            TopologyPatch::new(vec![IndexEdit::set_weight(0, 2, 1.1)]),
            TopologyPatch::new(vec![IndexEdit::activate(0, 1, 3.0)]),
        ];
        assert!(
            stream
                .apply_patches(&patches, CorrespondenceMode::Omit)
                .is_err()
        );
        assert_eq!(stream.sequence(), 0);
        assert_eq!(stream.current().version(), old_root);
    }
}
