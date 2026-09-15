use crate::ProofBar;
use crate::persistent_class::VerifiedPersistentClass;

use crate::cohomology::Edge;

/// A checked harmonic coordinate bound to one persistent H1 class.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedPersistentCoordinate {
    class: VerifiedPersistentClass,
    tolerance: f64,
    field_multiplier: u32,
    divisibility: u64,
    active_edges: usize,
    relative_residual: f64,
    integral: Vec<VerifiedIntegralTerm>,
    potential: Vec<f64>,
    payload_digest: [u8; 32],
}

/// One checked nonzero integer coefficient on an active edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VerifiedIntegralTerm {
    u: usize,
    v: usize,
    coefficient: i64,
}

impl VerifiedIntegralTerm {
    /// Return the lower edge endpoint.
    pub fn u(&self) -> usize {
        self.u
    }

    /// Return the higher edge endpoint.
    pub fn v(&self) -> usize {
        self.v
    }

    /// Return the checked integer coefficient.
    pub fn coefficient(&self) -> i64 {
        self.coefficient
    }
}

impl VerifiedPersistentCoordinate {
    /// Return the checked persistent-class artifact.
    pub fn class(&self) -> &VerifiedPersistentClass {
        &self.class
    }

    /// Return the number of labeled vertices in the checked source.
    pub fn vertex_count(&self) -> usize {
        self.class.vertex_count()
    }

    /// Return the number of source edges active at the representative scale.
    pub fn active_edges(&self) -> usize {
        self.active_edges
    }

    /// Return the checked coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.class.modulus()
    }

    /// Return the checked representative scale.
    pub fn scale(&self) -> f64 {
        self.class.scale()
    }

    /// Return the relative residual bound.
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// Return the checked field multiplier.
    pub fn field_multiplier(&self) -> u32 {
        self.field_multiplier
    }

    /// Return the recomputed integral divisibility.
    pub fn divisibility(&self) -> u64 {
        self.divisibility
    }

    /// Return the recomputed relative harmonic residual.
    pub fn relative_residual(&self) -> f64 {
        self.relative_residual
    }

    /// Return the checked integer lift terms.
    pub fn integral(&self) -> &[VerifiedIntegralTerm] {
        &self.integral
    }

    /// Return the checked gauge-fixed vertex potential.
    pub fn potential(&self) -> &[f64] {
        &self.potential
    }

    /// Return the canonical phase derived from the checked potential.
    pub fn phase(&self) -> Vec<f64> {
        self.potential
            .iter()
            .copied()
            .map(canonical_phase)
            .collect()
    }

    /// Return the checked persistent interval.
    pub fn interval(&self) -> ProofBar {
        self.class.interval()
    }

    /// Return the SHA-256 digest of the outer artifact payload.
    pub fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }
}

fn canonical_phase(value: f64) -> f64 {
    let phase = value.rem_euclid(1.0);
    if phase >= 1.0 || phase == 0.0 {
        0.0
    } else {
        phase
    }
}

pub(crate) struct DecodedPersistentCoordinate {
    pub(crate) nested_bytes: Vec<u8>,
    pub(crate) tolerance: f64,
    pub(crate) field_multiplier: u32,
    pub(crate) divisibility: u64,
    pub(crate) integral: Vec<(Edge, i64)>,
    pub(crate) potential: Vec<f64>,
    pub(crate) payload_digest: [u8; 32],
}

impl DecodedPersistentCoordinate {
    pub(crate) fn into_verified(
        self,
        class: VerifiedPersistentClass,
        active_edges: usize,
        relative_residual: f64,
    ) -> VerifiedPersistentCoordinate {
        let integral = self
            .integral
            .into_iter()
            .map(|(edge, coefficient)| VerifiedIntegralTerm {
                u: edge.u,
                v: edge.v,
                coefficient,
            })
            .collect();
        VerifiedPersistentCoordinate {
            class,
            tolerance: self.tolerance,
            field_multiplier: self.field_multiplier,
            divisibility: self.divisibility,
            active_edges,
            relative_residual,
            integral,
            potential: self.potential,
            payload_digest: self.payload_digest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_phase;

    #[test]
    fn canonical_phase_normalizes_wrap_boundaries() {
        assert_eq!(canonical_phase(-1e-17).to_bits(), 0.0f64.to_bits());
        assert_eq!(canonical_phase(-0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(canonical_phase(2.25), 0.25);
        assert_eq!(canonical_phase(-0.25), 0.75);
    }
}
