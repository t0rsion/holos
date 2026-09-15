use crate::{CriticalPair, PersistentClass, SparseDistanceMatrix};

/// One nonzero coefficient on an oriented edge of a geometric persistence
/// cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistenceCycleTerm {
    /// Lower endpoint of the edge.
    pub u: usize,
    /// Higher endpoint of the edge.
    pub v: usize,
    /// Coefficient in the declared prime field.
    pub coefficient: u32,
}

/// One nonzero coefficient on an oriented triangle of a finite death chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistenceTriangleTerm {
    /// Triangle vertices in ascending order.
    pub vertices: [usize; 3],
    /// Coefficient in the declared prime field.
    pub coefficient: u32,
}

/// A persistent H1 class with a geometric cycle witness.
///
/// The source graph, threshold, class, critical pair, and geometric terms
/// are private so callers cannot construct a claim without the producer
/// checks. Use [`Self::build`](crate::PersistentClassArtifact::build).
#[derive(Debug, Clone)]
pub struct PersistentClassArtifact {
    pub(super) source: SparseDistanceMatrix,
    pub(super) threshold: Option<f64>,
    pub(super) class: PersistentClass,
    pub(super) critical_pair: CriticalPair,
    pub(super) cycle: Vec<PersistenceCycleTerm>,
    pub(super) bounding_chain: Vec<PersistenceTriangleTerm>,
}

impl PersistentClassArtifact {
    /// The complete weighted source graph bound to this artifact.
    pub fn source(&self) -> &SparseDistanceMatrix {
        &self.source
    }

    /// The declared filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// The selected canonical persistent class.
    pub fn class(&self) -> &PersistentClass {
        &self.class
    }

    /// The critical pair selected from the class interval group.
    pub fn critical_pair(&self) -> &CriticalPair {
        &self.critical_pair
    }

    /// The normalized geometric cycle in ascending edge order.
    pub fn cycle(&self) -> &[PersistenceCycleTerm] {
        &self.cycle
    }

    /// The finite-death bounding chain in ascending triangle order.
    pub fn bounding_chain(&self) -> &[PersistenceTriangleTerm] {
        &self.bounding_chain
    }
}
