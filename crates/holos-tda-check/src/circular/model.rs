use crate::cohomology::{Edge, MapTerm, Space};
use crate::{ProofError, ProofLimits};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSCC\0";
pub(super) const VERSION: u16 = 1;
pub(super) const F64_BITS_CODEC: u8 = 1;

/// Limits for an independently checked circular-coordinate artifact.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CircularProofLimits {
    /// General byte, graph, simplex, and term limits.
    pub proof: ProofLimits,
    /// Largest accepted relative normal-equation residual tolerance.
    pub max_tolerance: f64,
    /// Largest absolute integer cocycle coefficient.
    pub max_integral_coefficient: u64,
}

impl Default for CircularProofLimits {
    fn default() -> Self {
        Self {
            proof: ProofLimits::default(),
            max_tolerance: 1e-8,
            max_integral_coefficient: 1u64 << 31,
        }
    }
}

/// Checked classification of a two-state circular continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCircularContinuationKind {
    /// The new restriction map has no kernel and one nonzero class matches.
    Unique,
    /// The new restriction map has a nonzero kernel.
    Ambiguous,
    /// No new class has the selected old restriction.
    NoExtension,
    /// Zero is the only new class with the selected old restriction.
    NoNonzeroContinuation,
}

/// Counts and numerical claims from one checked circular artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedCircularCoordinate {
    /// Prime field of the selected fixed-scale classes.
    pub modulus: u32,
    /// Fixed flag-complex scale carried by the class identifiers.
    pub scale: f64,
    /// Artifact residual tolerance, bounded by the checker limits.
    pub tolerance: f64,
    /// Graph states checked.
    pub states: usize,
    /// Harmonic coordinates checked.
    pub coordinates: usize,
    /// Total active edge count.
    pub edges: usize,
    /// Largest relative residual among the checked coordinates.
    pub max_relative_residual: f64,
    /// Integral-class divisibilities in coordinate order.
    pub divisibilities: Vec<u64>,
    /// Two-state continuation classification, when present.
    pub continuation: Option<VerifiedCircularContinuationKind>,
    /// Dimension of the new-side ambiguity direction.
    pub ambiguity_rank: usize,
}

#[derive(Debug)]
pub(super) struct Claim {
    pub(super) modulus: u32,
    pub(super) scale: f64,
    pub(super) tolerance: f64,
    pub(super) states: Vec<StateClaim>,
    pub(super) continuation: Option<ContinuationClaim>,
}

#[derive(Debug)]
pub(super) struct StateClaim {
    pub(super) vertex_count: usize,
    pub(super) edges: Vec<Edge>,
    pub(super) coordinate: Option<CoordinateClaim>,
}

#[derive(Debug)]
pub(super) struct CoordinateClaim {
    pub(super) space: [u8; 32],
    pub(super) field_multiplier: u32,
    pub(super) divisibility: u64,
    pub(super) source: Vec<(Edge, u32)>,
    pub(super) integral: Vec<(Edge, i64)>,
    pub(super) class: Vec<MapTerm>,
    pub(super) potential: Vec<f64>,
}

#[derive(Debug)]
pub(super) struct ContinuationClaim {
    pub(super) kind: VerifiedCircularContinuationKind,
    pub(super) target: Vec<MapTerm>,
    pub(super) ambiguity: Vec<Vec<MapTerm>>,
}

pub(crate) struct VerifiedCircularBinding {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<Edge>,
    pub(crate) modulus: u32,
    pub(crate) scale: f64,
    pub(crate) tolerance: f64,
    pub(crate) space: [u8; 32],
    pub(crate) class: Vec<MapTerm>,
}

pub(super) struct CheckedState {
    pub(super) space: Space,
    pub(super) relative_residual: Option<f64>,
    pub(super) divisibility: Option<u64>,
}

pub(super) struct ClaimHeader {
    pub(super) modulus: u32,
    pub(super) scale: f64,
    pub(super) tolerance: f64,
}

pub(super) struct CoordinateIdentity {
    pub(super) space: [u8; 32],
    pub(super) field_multiplier: u32,
    pub(super) divisibility: u64,
}

pub(super) struct CoordinateCounts {
    pub(super) source_count: usize,
    pub(super) integral_count: usize,
    pub(super) potential_count: usize,
}

impl CircularProofLimits {
    pub(super) fn validate(self) -> Result<(), ProofError> {
        if !self.max_tolerance.is_finite()
            || self.max_tolerance <= 0.0
            || self.max_integral_coefficient == 0
        {
            return Err(ProofError::new("circular checker limits are invalid"));
        }
        Ok(())
    }
}
