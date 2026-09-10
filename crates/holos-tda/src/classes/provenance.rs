use sha2::{Digest, Sha256};

use crate::{Bar, Cocycle, Error, Result, SparseDistanceMatrix};

use super::canonical::basis_class_id;
use super::model::{PersistentClass, PersistentClassProvenance};

impl PersistentClassProvenance {
    pub(crate) fn new(
        graph: &SparseDistanceMatrix,
        class_digest: [u8; 32],
        interval: Bar,
        cocycle: &Cocycle,
    ) -> Self {
        Self {
            source_graph_digest: source_graph_digest(graph, cocycle.scale),
            class_digest,
            interval,
            modulus: cocycle.modulus,
            scale: cocycle.scale,
        }
    }

    /// Construct a provenance record from previously recorded digests.
    ///
    /// `class_digest` must be the canonical [`crate::BasisClassId`] bytes for the
    /// class that uses this record.
    pub fn from_parts(
        source_graph_digest: [u8; 32],
        class_digest: [u8; 32],
        interval: Bar,
        modulus: u32,
        scale: f64,
    ) -> Self {
        Self {
            source_graph_digest,
            class_digest,
            interval,
            modulus,
            scale,
        }
    }

    /// Check the class identity, interval, and representative against a graph.
    ///
    /// The class digest must equal the canonical [`crate::BasisClassId`] bytes for
    /// the supplied cocycle.
    pub fn validate(
        &self,
        graph: &SparseDistanceMatrix,
        class_digest: [u8; 32],
        interval: Bar,
        cocycle: &Cocycle,
    ) -> Result<()> {
        self.validate_metadata(class_digest, interval, cocycle)?;
        if self.source_graph_digest != source_graph_digest(graph, self.scale) {
            return Err(Error::InvalidInput(
                "persistent class belongs to a different active graph".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_metadata(
        &self,
        class_digest: [u8; 32],
        interval: Bar,
        cocycle: &Cocycle,
    ) -> Result<()> {
        if self.class_digest != class_digest {
            return Err(Error::InvalidInput(
                "persistent class identity differs from its provenance".into(),
            ));
        }
        if !bar_bits_equal(self.interval, interval) {
            return Err(Error::InvalidInput(
                "persistent class interval differs from its provenance".into(),
            ));
        }
        if self.modulus != cocycle.modulus {
            return Err(Error::InvalidInput(
                "persistent class field differs from its provenance".into(),
            ));
        }
        if self.scale.to_bits() != cocycle.scale.to_bits() {
            return Err(Error::InvalidInput(
                "persistent class scale differs from its provenance".into(),
            ));
        }
        if !valid_interval_and_scale(interval, self.scale) {
            return Err(Error::InvalidInput(
                "persistent class representative is outside its interval".into(),
            ));
        }
        Ok(())
    }
}

impl PersistentClass {
    /// Source binding for a scalar Rips class, when available.
    pub fn provenance(&self) -> Option<&PersistentClassProvenance> {
        self.provenance.as_ref()
    }

    /// Check that this class belongs to the supplied active graph.
    pub fn validate_provenance(&self, graph: &SparseDistanceMatrix) -> Result<()> {
        let provenance = self.provenance.as_ref().ok_or_else(|| {
            Error::InvalidInput("persistent class has no interval-bound provenance".into())
        })?;
        provenance.validate(graph, *self.id.as_bytes(), self.interval, &self.cocycle)?;
        if basis_class_id(self.group_id, self.basis_index, &self.cocycle) != self.id {
            return Err(Error::InvalidInput(
                "persistent class identifier is not canonical".into(),
            ));
        }
        Ok(())
    }
}

fn valid_interval_and_scale(interval: Bar, scale: f64) -> bool {
    interval.dim == 1
        && valid_nonnegative_finite(interval.birth)
        && valid_death(interval.death)
        && (interval.death.is_infinite() || interval.death > interval.birth)
        && valid_nonnegative_finite(scale)
        && scale >= interval.birth
        && (interval.death.is_infinite() || scale < interval.death)
}

fn valid_nonnegative_finite(value: f64) -> bool {
    value.is_finite() && value >= 0.0 && !is_negative_zero(value)
}

fn valid_death(value: f64) -> bool {
    (value.is_finite() && value >= 0.0 && !is_negative_zero(value))
        || (value.is_infinite() && value.is_sign_positive())
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

pub(crate) fn source_graph_digest(graph: &SparseDistanceMatrix, scale: f64) -> [u8; 32] {
    let active = graph
        .edges()
        .filter(|&(_, _, value)| value <= scale)
        .collect::<Vec<_>>();
    let mut hash = Sha256::new();
    hash.update(b"holos-persistent-class-source-v1");
    hash.update((graph.len() as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    hash.update((active.len() as u64).to_be_bytes());
    for (u, v, value) in active {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn bar_bits_equal(a: Bar, b: Bar) -> bool {
    a.dim == b.dim
        && a.birth.to_bits() == b.birth.to_bits()
        && a.death.to_bits() == b.death.to_bits()
}
