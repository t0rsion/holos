use crate::ProofBar;

/// One source edge carried by a checked persistent-class artifact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PersistentSourceEdge {
    pub(crate) u: usize,
    pub(crate) v: usize,
    pub(crate) value: f64,
}

impl PersistentSourceEdge {
    /// Return the lower endpoint.
    pub fn u(&self) -> usize {
        self.u
    }

    /// Return the higher endpoint.
    pub fn v(&self) -> usize {
        self.v
    }

    /// Return the source edge weight.
    pub fn value(&self) -> f64 {
        self.value
    }
}

/// One finite-field term of a checked persistent cocycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistentCocycleTerm {
    pub(crate) u: usize,
    pub(crate) v: usize,
    pub(crate) coefficient: u32,
}

impl PersistentCocycleTerm {
    /// Return the lower endpoint.
    pub fn u(&self) -> usize {
        self.u
    }

    /// Return the higher endpoint.
    pub fn v(&self) -> usize {
        self.v
    }

    /// Return the finite-field coefficient.
    pub fn coefficient(&self) -> u32 {
        self.coefficient
    }
}

/// One finite-field term of a checked birth cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistenceCycleTerm {
    pub(crate) u: usize,
    pub(crate) v: usize,
    pub(crate) coefficient: u32,
}

impl PersistenceCycleTerm {
    /// Return the lower endpoint.
    pub fn u(&self) -> usize {
        self.u
    }

    /// Return the higher endpoint.
    pub fn v(&self) -> usize {
        self.v
    }

    /// Return the finite-field coefficient.
    pub fn coefficient(&self) -> u32 {
        self.coefficient
    }
}

/// One finite-field term of a checked death bounding chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistenceTriangleTerm {
    pub(crate) vertices: [usize; 3],
    pub(crate) coefficient: u32,
}

impl PersistenceTriangleTerm {
    /// Return the triangle vertices in ascending order.
    pub fn vertices(&self) -> [usize; 3] {
        self.vertices
    }

    /// Return the finite-field coefficient.
    pub fn coefficient(&self) -> u32 {
        self.coefficient
    }
}

/// The critical pair selected by a checked persistent-class artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistentCriticalPair {
    pub(crate) birth: [usize; 2],
    pub(crate) death: Option<[usize; 3]>,
}

impl PersistentCriticalPair {
    /// Return the birth edge.
    pub fn birth(&self) -> [usize; 2] {
        self.birth
    }

    /// Return the optional death triangle.
    pub fn death(&self) -> Option<[usize; 3]> {
        self.death
    }
}

/// A class selected and checked by the independent persistent-class verifier.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedPersistentClass {
    vertex_count: usize,
    source: Vec<PersistentSourceEdge>,
    threshold: Option<f64>,
    modulus: u32,
    group_id: [u8; 32],
    class_id: [u8; 32],
    basis_index: usize,
    interval: ProofBar,
    scale: f64,
    cocycle: Vec<PersistentCocycleTerm>,
    pair: PersistentCriticalPair,
    cycle: Vec<PersistenceCycleTerm>,
    bounding_chain: Vec<PersistenceTriangleTerm>,
    payload_digest: [u8; 32],
}

impl VerifiedPersistentClass {
    /// Return the number of labeled source vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Return the complete checked weighted source graph.
    pub fn source(&self) -> &[PersistentSourceEdge] {
        &self.source
    }

    /// Return the threshold used for the checked filtration.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Return the checked coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Return the checked interval-group identifier.
    pub fn group_id(&self) -> &[u8; 32] {
        &self.group_id
    }

    /// Return the checked class identifier.
    pub fn class_id(&self) -> &[u8; 32] {
        &self.class_id
    }

    /// Return the checked position in the canonical group basis.
    pub fn basis_index(&self) -> usize {
        self.basis_index
    }

    /// Return the checked persistence interval.
    pub fn interval(&self) -> ProofBar {
        self.interval
    }

    /// Return the checked representative scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Return the exact canonical cocycle selected by replay.
    pub fn cocycle(&self) -> &[PersistentCocycleTerm] {
        &self.cocycle
    }

    /// Return the checked critical pair.
    pub fn critical_pair(&self) -> PersistentCriticalPair {
        self.pair
    }

    /// Return the checked birth cycle.
    pub fn cycle(&self) -> &[PersistenceCycleTerm] {
        &self.cycle
    }

    /// Return the checked death bounding chain.
    pub fn bounding_chain(&self) -> &[PersistenceTriangleTerm] {
        &self.bounding_chain
    }

    /// Return the SHA-256 digest of the checked artifact payload.
    pub fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }
}

pub(crate) struct DecodedPersistentClass {
    pub(crate) vertex_count: usize,
    pub(crate) source: Vec<PersistentSourceEdge>,
    pub(crate) threshold: Option<f64>,
    pub(crate) modulus: u32,
    pub(crate) group_id: [u8; 32],
    pub(crate) class_id: [u8; 32],
    pub(crate) basis_index: usize,
    pub(crate) interval: ProofBar,
    pub(crate) scale: f64,
    pub(crate) cocycle: Vec<PersistentCocycleTerm>,
    pub(crate) pair: PersistentCriticalPair,
    pub(crate) cycle: Vec<PersistenceCycleTerm>,
    pub(crate) bounding_chain: Vec<PersistenceTriangleTerm>,
    pub(crate) payload_digest: [u8; 32],
}

impl DecodedPersistentClass {
    pub(crate) fn into_verified(self) -> VerifiedPersistentClass {
        VerifiedPersistentClass {
            vertex_count: self.vertex_count,
            source: self.source,
            threshold: self.threshold,
            modulus: self.modulus,
            group_id: self.group_id,
            class_id: self.class_id,
            basis_index: self.basis_index,
            interval: self.interval,
            scale: self.scale,
            cocycle: self.cocycle,
            pair: self.pair,
            cycle: self.cycle,
            bounding_chain: self.bounding_chain,
            payload_digest: self.payload_digest,
        }
    }
}
