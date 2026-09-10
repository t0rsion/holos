use std::fmt;

use crate::{
    AtlasArtifact, AtlasDecodeLimits, AtlasEvaluation, CertificateLimits, RipsParams,
    SparseDistanceMatrix, TopologyEvent, UpdateMode,
};

use super::{decode, encode, verification};

/// Failure while producing, decoding, or checking a trajectory artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryError {
    message: String,
}

impl TrajectoryError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated trajectory rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TrajectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trajectory artifact: {}", self.message)
    }
}

impl std::error::Error for TrajectoryError {}

/// Decoder limits applied before trajectory collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TrajectoryDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted update count.
    pub max_steps: usize,
    /// Largest vertex count in one graph.
    pub max_vertices: usize,
    /// Largest total edge count across all graphs.
    pub max_total_edges: usize,
    /// Largest total event count.
    pub max_total_events: usize,
    /// Largest nested atlas envelope in bytes.
    pub max_atlas_bytes: usize,
}

impl Default for TrajectoryDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_steps: 10_000_000,
            max_vertices: 1_000_000,
            max_total_edges: 200_000_000,
            max_total_events: 100_000_000,
            max_atlas_bytes: 1 << 30,
        }
    }
}

/// One graph and its transition from the preceding atlas region.
#[derive(Debug, Clone)]
pub struct TrajectoryStep {
    pub(super) input: SparseDistanceMatrix,
    pub(super) mode: UpdateMode,
    pub(super) events: Vec<TopologyEvent>,
    pub(super) checkpoint: Option<AtlasArtifact>,
}

impl TrajectoryStep {
    /// Graph evaluated at this step.
    pub fn input(&self) -> &SparseDistanceMatrix {
        &self.input
    }

    /// Transition from the prior atlas region.
    pub fn mode(&self) -> UpdateMode {
        self.mode
    }

    /// Events declared at this step.
    pub fn events(&self) -> &[TopologyEvent] {
        &self.events
    }

    /// Atlas checkpoint at a region boundary.
    pub fn checkpoint(&self) -> Option<&AtlasArtifact> {
        self.checkpoint.as_ref()
    }
}

/// Graphs, events, and proofs for a persistence trajectory.
#[derive(Debug, Clone)]
pub struct TrajectoryArtifact {
    pub(super) initial_input: SparseDistanceMatrix,
    pub(super) initial_atlas: AtlasArtifact,
    pub(super) steps: Vec<TrajectoryStep>,
}

impl TrajectoryArtifact {
    /// Compile and certify a sequence of sparse weighted graphs.
    pub fn build(
        initial: &SparseDistanceMatrix,
        updates: &[SparseDistanceMatrix],
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, TrajectoryError> {
        let (initial_atlas, mut atlas) =
            AtlasArtifact::compile(initial, params, certificate_limits)
                .map_err(|error| TrajectoryError::new(error.to_string()))?;
        let mut steps = Vec::with_capacity(updates.len());
        for input in updates {
            let events = atlas.events(input);
            if events.is_empty() {
                atlas
                    .evaluate(input)
                    .map_err(|error| TrajectoryError::new(error.to_string()))?;
                steps.push(TrajectoryStep {
                    input: input.clone(),
                    mode: UpdateMode::Reused,
                    events,
                    checkpoint: None,
                });
            } else {
                let (checkpoint, next) = AtlasArtifact::compile(input, params, certificate_limits)
                    .map_err(|error| TrajectoryError::new(error.to_string()))?;
                atlas = next;
                steps.push(TrajectoryStep {
                    input: input.clone(),
                    mode: UpdateMode::Recomputed,
                    events,
                    checkpoint: Some(checkpoint),
                });
            }
        }
        Ok(Self {
            initial_input: initial.clone(),
            initial_atlas,
            steps,
        })
    }

    /// Initial graph.
    pub fn initial_input(&self) -> &SparseDistanceMatrix {
        &self.initial_input
    }

    /// Initial atlas.
    pub fn initial_atlas(&self) -> &AtlasArtifact {
        &self.initial_atlas
    }

    /// Ordered trajectory steps.
    pub fn steps(&self) -> &[TrajectoryStep] {
        &self.steps
    }

    /// Encode the canonical `HOLOSTRC` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, TrajectoryError> {
        self.check_shape()?;
        let initial_atlas = encode::encode_atlas(&self.initial_atlas)?;
        let checkpoints = encode::encode_checkpoints(&self.steps)?;
        let mut out = Vec::new();
        encode::encode_trajectory_header(&mut out, self.steps.len(), initial_atlas.len())?;
        encode::encode_graph(&mut out, &self.initial_input)?;
        out.extend_from_slice(&initial_atlas);
        for (step, checkpoint) in self.steps.iter().zip(checkpoints) {
            encode::encode_trajectory_step(&mut out, step, checkpoint.as_deref())?;
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded trajectory envelope.
    pub fn decode(
        bytes: &[u8],
        limits: TrajectoryDecodeLimits,
        atlas_limits: AtlasDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, TrajectoryError> {
        decode::check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = super::primitives::Reader::new(bytes);
        let header = decode::decode_trajectory_header(&mut reader, limits)?;
        let mut total_edges = 0usize;
        let initial_input = decode::decode_graph(&mut reader, limits, &mut total_edges)?;
        let initial_atlas = decode::decode_nested_atlas(
            &mut reader,
            header.initial_atlas_bytes,
            limits,
            atlas_limits,
            certificate_limits,
        )?;
        let mut total_events = 0usize;
        let mut context = decode::TrajectoryDecodeContext {
            limits,
            atlas_limits,
            certificate_limits,
            total_edges: &mut total_edges,
            total_events: &mut total_events,
        };
        let steps = decode::decode_trajectory_steps(&mut reader, header.step_count, &mut context)?;
        decode::check_no_trailing_bytes(&reader)?;
        let artifact = Self {
            initial_input,
            initial_atlas,
            steps,
        };
        artifact.check_shape()?;
        Ok(artifact)
    }

    /// Verify all proofs and region transitions.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedTrajectory, TrajectoryError> {
        self.check_shape()?;
        let mut atlas = verification::verify_atlas(
            &self.initial_atlas,
            &self.initial_input,
            certificate_limits,
        )?;
        let initial = atlas
            .evaluate(&self.initial_input)
            .map_err(|error| TrajectoryError::new(error.to_string()))?;
        let mut verified_steps = Vec::with_capacity(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            verified_steps.push(verification::verify_trajectory_step(
                &mut atlas,
                step,
                index,
                certificate_limits,
            )?);
        }
        Ok(VerifiedTrajectory {
            initial,
            steps: verified_steps,
        })
    }

    fn check_shape(&self) -> std::result::Result<(), TrajectoryError> {
        for (index, step) in self.steps.iter().enumerate() {
            let checkpoint_matches = matches!(
                (step.mode, step.checkpoint.is_some()),
                (UpdateMode::Reused, false) | (UpdateMode::Recomputed, true)
            );
            if !checkpoint_matches {
                return Err(TrajectoryError::new(format!(
                    "step {index} checkpoint does not match its mode"
                )));
            }
            if (step.mode == UpdateMode::Reused) != step.events.is_empty() {
                return Err(TrajectoryError::new(format!(
                    "step {index} event count does not match its mode"
                )));
            }
        }
        Ok(())
    }
}

/// Evaluation at one trajectory step.
#[derive(Debug, Clone)]
pub struct VerifiedTrajectoryStep {
    /// Transition mode.
    pub mode: UpdateMode,
    /// Events derived from the preceding atlas.
    pub events: Vec<TopologyEvent>,
    /// Exact H0 and H1 result.
    pub evaluation: AtlasEvaluation,
}

/// Results reconstructed from a trajectory artifact.
#[derive(Debug, Clone)]
pub struct VerifiedTrajectory {
    /// Exact result for the initial graph.
    pub initial: AtlasEvaluation,
    /// Update results.
    pub steps: Vec<VerifiedTrajectoryStep>,
}
