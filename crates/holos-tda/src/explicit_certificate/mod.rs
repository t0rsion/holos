//! Proof-carrying persistence for explicit scalar filtered complexes.
//!
//! The caller supplies every simplex and face as a
//! [`FilteredSimplicialComplex`]. The certificate records a
//! filtration-compatible unit-triangular basis change in each boundary
//! dimension. The diagram is derived from checked pivots.

use crate::Diagram;
use crate::certificate::{CertificateError, CertificateLimits, ChangeColumn};
use crate::filtration::{FilteredSimplicialComplex, ScalarGrade};
use crate::graded_certificate::{GradedComplex, check_all, reduce_all_dimensions};

mod validation;
mod wire;

#[cfg(all(test, holos_repository_tests))]
mod tests;

use validation::{certificate_error, diagrams_equal, truncate_complex, validate_parameters};

/// A dimension-generic `D V = R` certificate for an explicit filtration.
#[derive(Debug, Clone)]
pub struct ExplicitReductionCertificate {
    complex: FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    modulus: u32,
    columns: Vec<Vec<ChangeColumn>>,
    diagram: Diagram,
    digest: [u8; 32],
}

impl ExplicitReductionCertificate {
    /// Build and check an explicit reduction certificate.
    ///
    /// The complex must contain simplex groups through dimension
    /// `max_homology_dimension + 1`. An empty group records that no cofaces
    /// occur in that dimension.
    pub fn build(
        complex: &FilteredSimplicialComplex<ScalarGrade>,
        max_homology_dimension: usize,
        modulus: u32,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        validate_parameters(complex, max_homology_dimension, modulus, limits)?;
        let complex = truncate_complex(complex, max_homology_dimension)?;
        let ordered = GradedComplex::from_filtered(&complex, max_homology_dimension, limits)?;
        let columns = reduce_all_dimensions(&ordered, max_homology_dimension, modulus, limits)?;
        let checked = check_all(&ordered, modulus, &columns, limits)?;
        let mut certificate = Self {
            complex,
            max_homology_dimension,
            modulus,
            columns,
            diagram: checked.diagram,
            digest: [0; 32],
        };
        certificate.digest = certificate.compute_digest()?;
        certificate.verify(limits)?;
        Ok(certificate)
    }

    /// Explicit complex bound to the proof.
    pub fn complex(&self) -> &FilteredSimplicialComplex<ScalarGrade> {
        &self.complex
    }

    /// Highest homology dimension.
    pub fn max_homology_dimension(&self) -> usize {
        self.max_homology_dimension
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Unit-triangular change columns by boundary dimension.
    pub fn columns(&self) -> &[Vec<ChangeColumn>] {
        &self.columns
    }

    /// Diagram derived from the checked reductions.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Content digest of the payload.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Verify the filtration, every `D V = R` relation, unique pivots, and the diagram.
    pub fn verify(&self, limits: CertificateLimits) -> Result<(), CertificateError> {
        validate_parameters(
            &self.complex,
            self.max_homology_dimension,
            self.modulus,
            limits,
        )?;
        if self.columns.len() != self.max_homology_dimension + 1 {
            return Err(certificate_error(
                "boundary-dimension count differs from the requested range",
            ));
        }
        let ordered =
            GradedComplex::from_filtered(&self.complex, self.max_homology_dimension, limits)?;
        let checked = check_all(&ordered, self.modulus, &self.columns, limits)?;
        if !diagrams_equal(&checked.diagram, &self.diagram) {
            return Err(certificate_error(
                "diagram differs from the checked explicit reductions",
            ));
        }
        if self.compute_digest()? != self.digest {
            return Err(certificate_error(
                "explicit certificate digest does not match",
            ));
        }
        Ok(())
    }

    /// Encode canonical `HOLOSEXP` version 1 bytes.
    pub fn encode(&self, limits: CertificateLimits) -> Result<Vec<u8>, CertificateError> {
        self.verify(limits)?;
        let mut output = wire::encode_payload(self)?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(certificate_error(
                "explicit certificate exceeds its byte limit",
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSEXP` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: CertificateLimits) -> Result<Self, CertificateError> {
        let (payload, digest) = wire::decode_digest(bytes, limits.max_bytes)?;
        let decoded = wire::decode_payload(payload, digest, limits)?;
        decoded.verify(limits)?;
        Ok(decoded)
    }

    fn compute_digest(&self) -> Result<[u8; 32], CertificateError> {
        wire::compute_digest(self)
    }
}
