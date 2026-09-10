use std::fmt;

use crate::{
    Bar, ExplainedDiagram, IntervalGroupId, ReductionCertificate, ReductionRepairMode,
    ReductionRepairWork,
};

/// Failure while producing, decoding, or checking an atlas artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasArtifactError {
    message: String,
}

impl AtlasArtifactError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated artifact rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AtlasArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "atlas artifact: {}", self.message)
    }
}

impl std::error::Error for AtlasArtifactError {}

/// Decoder limits applied before atlas collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AtlasDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted bar count.
    pub max_bars: usize,
    /// Largest accepted class-space count.
    pub max_spaces: usize,
    /// Largest accepted total basis count.
    pub max_basis: usize,
    /// Largest accepted total critical-pair count.
    pub max_critical_pairs: usize,
    /// Largest accepted total cocycle term count.
    pub max_terms: usize,
    /// Largest accepted nested reduction certificate in bytes.
    pub max_certificate_bytes: usize,
}

impl Default for AtlasDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_bars: 100_000_000,
            max_spaces: 50_000_000,
            max_basis: 100_000_000,
            max_critical_pairs: 100_000_000,
            max_terms: 200_000_000,
            max_certificate_bytes: 1 << 30,
        }
    }
}

/// Input binding, class atlas, and nested reduction certificate.
#[derive(Debug, Clone)]
pub struct AtlasArtifact {
    pub(crate) vertex_count: usize,
    pub(crate) threshold: Option<f64>,
    pub(crate) modulus: u32,
    pub(crate) input_digest: [u8; 32],
    pub(crate) explained: ExplainedDiagram,
    pub(crate) reduction: ReductionCertificate,
}

pub(crate) struct AtlasHeader {
    pub(crate) modulus: u32,
    pub(crate) vertex_count: usize,
    pub(crate) threshold: Option<f64>,
    pub(crate) bars: usize,
    pub(crate) spaces: usize,
    pub(crate) certificate_bytes: usize,
    pub(crate) input_digest: [u8; 32],
}

#[derive(Default)]
pub(crate) struct AtlasTotals {
    pub(crate) basis: usize,
    pub(crate) critical_pairs: usize,
    pub(crate) terms: usize,
}

pub(crate) struct SpaceHeader {
    pub(crate) id: IntervalGroupId,
    pub(crate) interval: Bar,
    pub(crate) basis: usize,
    pub(crate) critical_pairs: usize,
}

/// An atlas after reduction repair.
#[derive(Debug, Clone)]
pub struct AtlasArtifactRepair {
    pub(crate) artifact: AtlasArtifact,
    pub(crate) mode: ReductionRepairMode,
    pub(crate) work: ReductionRepairWork,
}

impl AtlasArtifactRepair {
    /// Updated atlas.
    pub fn artifact(&self) -> &AtlasArtifact {
        &self.artifact
    }

    /// How the reduction was adapted.
    pub fn mode(&self) -> ReductionRepairMode {
        self.mode
    }

    /// Exact boundary-column work performed by the repair.
    pub fn work(&self) -> ReductionRepairWork {
        self.work
    }

    pub(crate) fn into_artifact(self) -> AtlasArtifact {
        self.artifact
    }
}
