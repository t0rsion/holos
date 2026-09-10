//! Wire format for checked circular coordinates and continuation.

mod api;
mod wire;

#[cfg(all(test, holos_repository_tests))]
mod tests;

use std::fmt;

use crate::{CircularClassTerm, CircularCoordinate, CohomologyContinuationKind};

const MAGIC: &[u8; 8] = b"HOLOSCC\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while constructing or encoding a circular-coordinate artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircularArtifactError {
    message: String,
}

impl CircularArtifactError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated artifact rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CircularArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "circular artifact: {}", self.message)
    }
}

impl std::error::Error for CircularArtifactError {}

/// Structural counts for one circular-coordinate artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircularArtifactSummary {
    /// Graph states carried by the artifact.
    pub states: usize,
    /// States that carry a harmonic coordinate.
    pub coordinates: usize,
    /// Total active edge count.
    pub edges: usize,
    /// Whether the artifact carries a two-state continuation claim.
    pub continuation: bool,
}

#[derive(Debug, Clone)]
struct ArtifactState {
    vertex_count: usize,
    edges: Vec<(usize, usize)>,
    coordinate: Option<CircularCoordinate>,
}

#[derive(Debug, Clone)]
struct ArtifactContinuation {
    kind: CohomologyContinuationKind,
    target: Vec<CircularClassTerm>,
    ambiguity: Vec<Vec<CircularClassTerm>>,
}

/// Self-contained `HOLOSCC` circular-coordinate artifact.
#[derive(Debug, Clone)]
pub struct CircularCoordinateArtifact {
    modulus: u32,
    scale: f64,
    tolerance: f64,
    states: Vec<ArtifactState>,
    continuation: Option<ArtifactContinuation>,
}
