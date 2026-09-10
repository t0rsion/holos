use std::fmt;

use crate::{Bar, Diagram};

/// Identifier of a persistent interval group and its class space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IntervalGroupId([u8; 32]);

impl IntervalGroupId {
    /// Identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Construct an identifier from its serialized bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for IntervalGroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Identifier of one vector in a declared canonical class-space basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BasisClassId([u8; 32]);

impl BasisClassId {
    /// Identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Construct an identifier from its serialized bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for BasisClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// One nonzero coefficient on an oriented edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CocycleTerm {
    /// Lower endpoint. The edge is oriented from `u` to `v`.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Canonical H1 cocycle at one filtration scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Cocycle {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Scale at which the terms represent the class.
    pub scale: f64,
    /// Nonzero terms in ascending endpoint order. The first coefficient is
    /// one.
    pub terms: Vec<CocycleTerm>,
}

/// One simplex that creates or destroys a persistence interval.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalSimplex {
    /// Vertices in ascending order.
    pub vertices: Vec<usize>,
    /// Filtration value of the simplex.
    pub value: f64,
}

/// Creator and optional destroyer of one generated interval.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalPair {
    /// Edge that creates the H1 interval.
    pub birth: CriticalSimplex,
    /// Triangle that destroys a finite H1 interval.
    pub death: Option<CriticalSimplex>,
}

/// Source binding for one interval-bound H1 class.
///
/// The source digest covers the active labeled graph at the representative
/// scale. The class digest is the canonical [`BasisClassId`] bytes. Derived
/// class spaces can omit this binding when their source graph is not the
/// scalar Rips input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PersistentClassProvenance {
    pub(crate) source_graph_digest: [u8; 32],
    pub(crate) class_digest: [u8; 32],
    pub(crate) interval: Bar,
    pub(crate) modulus: u32,
    pub(crate) scale: f64,
}

impl PersistentClassProvenance {
    /// Raw digest of the active labeled graph at the representative scale.
    pub fn source_graph_digest(&self) -> &[u8; 32] {
        &self.source_graph_digest
    }

    /// Deterministic digest of the canonical class identity.
    pub fn class_digest(&self) -> &[u8; 32] {
        &self.class_digest
    }

    /// Persistence interval bound to the source graph.
    pub fn interval(&self) -> Bar {
        self.interval
    }

    /// Prime field bound to the representative.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Representative scale bound to the interval.
    pub fn scale(&self) -> f64 {
        self.scale
    }
}

/// One vector in the declared basis of a persistent H1 class space.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentClass {
    /// Identifier derived from the class space, basis position, and cocycle.
    pub id: BasisClassId,
    /// Class space that contains this basis vector.
    pub group_id: IntervalGroupId,
    /// Position in the canonical basis of the class space.
    pub basis_index: usize,
    /// H1 persistence interval.
    pub interval: Bar,
    /// Representative on the caller's graph.
    pub cocycle: Cocycle,
    /// Source binding for scalar Rips classes, when available.
    pub provenance: Option<PersistentClassProvenance>,
}

/// All H1 classes with the same interval, represented as one class space.
///
/// Equal intervals do not have intrinsic individual identities. `basis` is
/// a deterministic row-reduced basis tied to the labeled input graph.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistentClassSpace {
    /// Identifier derived from the interval and canonical basis.
    pub id: IntervalGroupId,
    /// Shared H1 persistence interval.
    pub interval: Bar,
    /// Canonical basis of this class space.
    pub basis: Vec<PersistentClass>,
    /// Creator and destroyer pairs from the fixed reduction before basis
    /// canonicalization.
    pub critical_pairs: Vec<CriticalPair>,
}

/// Diagram plus canonical spaces for every positive H1 interval.
#[derive(Debug, Clone)]
pub struct ExplainedDiagram {
    /// Full persistence diagram through the requested dimension.
    pub diagram: Diagram,
    /// H1 class spaces in interval order, then identifier order.
    pub spaces: Vec<PersistentClassSpace>,
}

impl ExplainedDiagram {
    /// Number of positive H1 intervals, including multiplicity.
    pub fn class_count(&self) -> usize {
        self.spaces.iter().map(|space| space.basis.len()).sum()
    }

    /// Basis vectors from every class space in canonical order.
    pub fn classes(&self) -> impl Iterator<Item = &PersistentClass> {
        self.spaces.iter().flat_map(|space| &space.basis)
    }
}
