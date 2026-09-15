use std::fmt;

use crate::circular::SelectedCircularCoordinate;
use crate::{
    Bar, CriticalPair, IntegralCocycleTerm, PersistenceCycleTerm, PersistenceTriangleTerm,
    PersistentClass, PersistentClassArtifact, SparseDistanceMatrix,
};

/// Failure while constructing or encoding a selected-coordinate artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentCoordinateArtifactError {
    pub(super) message: String,
}

impl PersistentCoordinateArtifactError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Return the violated artifact rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PersistentCoordinateArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "persistent coordinate artifact: {}",
            self.message
        )
    }
}

impl std::error::Error for PersistentCoordinateArtifactError {}

/// Structural counts for one selected persistent-coordinate artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistentCoordinateArtifactSummary {
    /// Number of labeled source vertices.
    pub vertices: usize,
    /// Number of weighted source edges.
    pub source_edges: usize,
    /// Number of nonzero integer lift terms.
    pub integral_terms: usize,
    /// Number of potential values.
    pub potential_values: usize,
}

/// A harmonic coordinate bound to one checked persistent H1 class artifact.
///
/// The class artifact supplies the complete weighted source, interval, exact
/// canonical cocycle, and birth cycle. This type stores only the selected
/// coordinate's integer lift and harmonic values.
#[derive(Debug, Clone)]
pub struct PersistentCoordinateArtifact {
    pub(super) class_artifact: PersistentClassArtifact,
    pub(super) coordinate: SelectedCircularCoordinate,
}

impl PersistentCoordinateArtifact {
    /// Return the immutable persistent-class artifact carried by this result.
    pub fn class_artifact(&self) -> &PersistentClassArtifact {
        &self.class_artifact
    }

    /// Return the complete weighted source graph.
    pub fn source(&self) -> &SparseDistanceMatrix {
        self.class_artifact.source()
    }

    /// Return the selected persistent class.
    pub fn class(&self) -> &PersistentClass {
        self.class_artifact.class()
    }

    /// Return the selected persistence interval.
    pub fn interval(&self) -> Bar {
        self.class().interval
    }

    /// Return the selected critical pair.
    pub fn critical_pair(&self) -> &CriticalPair {
        self.class_artifact.critical_pair()
    }

    /// Return the checked birth cycle.
    pub fn cycle(&self) -> &[PersistenceCycleTerm] {
        self.class_artifact.cycle()
    }

    /// Return the checked finite-death bounding chain.
    pub fn bounding_chain(&self) -> &[PersistenceTriangleTerm] {
        self.class_artifact.bounding_chain()
    }

    /// Return the prime field of the selected source.
    pub fn modulus(&self) -> u32 {
        self.coordinate.modulus
    }

    /// Return the fixed representative scale.
    pub fn scale(&self) -> f64 {
        self.coordinate.scale
    }

    /// Return the nonzero field multiplier used by the integral lift.
    pub fn field_multiplier(&self) -> u32 {
        self.coordinate.field_multiplier
    }

    /// Return the checked integer cocycle used for the harmonic coordinate.
    pub fn integral(&self) -> &[IntegralCocycleTerm] {
        &self.coordinate.integral
    }

    /// Return the divisibility of the integer cohomology class.
    pub fn divisibility(&self) -> u64 {
        self.coordinate.divisibility
    }

    /// Return the gauge-fixed real vertex potential.
    pub fn potential(&self) -> &[f64] {
        &self.coordinate.potential
    }

    /// Return the circle-valued phase at each labeled vertex.
    pub fn phase(&self) -> &[f64] {
        &self.coordinate.phase
    }

    /// Return the squared unweighted harmonic energy.
    pub fn energy(&self) -> f64 {
        self.coordinate.energy
    }

    /// Return the maximum absolute harmonic residual.
    pub fn max_residual(&self) -> f64 {
        self.coordinate.max_residual
    }

    /// Return the residual divided by the source infinity norm.
    pub fn relative_residual(&self) -> f64 {
        self.coordinate.relative_residual
    }

    /// Return the conjugate-gradient iteration count.
    pub fn iterations(&self) -> usize {
        self.coordinate.iterations
    }

    /// Return the residual tolerance used by the producer.
    pub fn tolerance(&self) -> f64 {
        self.coordinate.tolerance
    }

    /// Return structural counts without encoding the artifact.
    pub fn summary(&self) -> PersistentCoordinateArtifactSummary {
        PersistentCoordinateArtifactSummary {
            vertices: self.source().len(),
            source_edges: self.source().num_edges(),
            integral_terms: self.integral().len(),
            potential_values: self.potential().len(),
        }
    }
}
