//! Certificates for exact kinetic cohomology zigzags.

mod wire;

#[cfg(test)]
mod tests;

use crate::{
    CohomologyLimits, Error, KineticEdge, KineticFiltration, KineticLimits, KineticZigzag, Result,
    ZigzagLimits,
};

/// Resource limits for kinetic zigzag artifacts and replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KineticZigzagArtifactLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Limits for the exact affine event schedule.
    pub kinetic: KineticLimits,
    /// Limits for each canonical cohomology computation.
    pub cohomology: CohomologyLimits,
    /// Limits for finite zigzag decomposition.
    pub zigzag: ZigzagLimits,
}

impl Default for KineticZigzagArtifactLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            kinetic: KineticLimits::default(),
            cohomology: CohomologyLimits::default(),
            zigzag: ZigzagLimits::default(),
        }
    }
}

/// One claimed interval-isotypic space in a kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KineticZigzagIntervalClaim {
    /// First zigzag node covered by the interval.
    pub start: usize,
    /// Last zigzag node covered by the interval, inclusive.
    pub end: usize,
    /// Number of indistinguishable copies.
    pub multiplicity: usize,
}

/// Size summary of a kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KineticZigzagArtifactSummary {
    /// Affine edge trajectory count.
    pub edges: usize,
    /// Alternating open-cell and event node count.
    pub nodes: usize,
    /// Exact restriction arrow count.
    pub arrows: usize,
    /// Nonzero interval-isotypic space count.
    pub intervals: usize,
    /// Sum of all interval multiplicities.
    pub interval_copies: usize,
}

/// Exact kinetic zigzag certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticZigzagArtifact {
    vertex_count: usize,
    edges: Vec<KineticEdge>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    persistent_ties: usize,
    node_ranks: Vec<usize>,
    node_active_edges: Vec<usize>,
    arrow_ranks: Vec<usize>,
    generalized_ranks: Vec<usize>,
    intervals: Vec<KineticZigzagIntervalClaim>,
    digest: [u8; 32],
}

impl KineticZigzagArtifact {
    /// Build one artifact and return the checked zigzag.
    pub fn build(
        trajectory: &KineticFiltration,
        dimension: usize,
        scale: f64,
        modulus: u32,
        limits: KineticZigzagArtifactLimits,
    ) -> Result<(Self, KineticZigzag)> {
        let zigzag = trajectory.cohomology_zigzag(
            dimension,
            scale,
            modulus,
            limits.cohomology,
            limits.zigzag,
        )?;
        let mut artifact = Self::from_zigzag(trajectory, &zigzag);
        artifact.digest = artifact.compute_digest()?;
        Ok((artifact, zigzag))
    }

    fn from_zigzag(trajectory: &KineticFiltration, zigzag: &KineticZigzag) -> Self {
        Self {
            vertex_count: trajectory.vertex_count(),
            edges: trajectory.edges().to_vec(),
            start: trajectory.start(),
            end: trajectory.end(),
            dimension: zigzag.dimension,
            scale: zigzag.scale,
            modulus: zigzag.modulus,
            persistent_ties: zigzag.persistent_ties,
            node_ranks: zigzag.nodes.iter().map(|node| node.rank).collect(),
            node_active_edges: zigzag.nodes.iter().map(|node| node.active_edges).collect(),
            arrow_ranks: zigzag
                .arrows
                .iter()
                .map(|arrow| arrow.restriction.rank)
                .collect(),
            generalized_ranks: zigzag.barcode.generalized_ranks.clone(),
            intervals: zigzag
                .barcode
                .intervals
                .iter()
                .map(|interval| KineticZigzagIntervalClaim {
                    start: interval.start,
                    end: interval.end,
                    multiplicity: interval.multiplicity,
                })
                .collect(),
            digest: [0; 32],
        }
    }

    /// Recompute the zigzag and compare every stored claim.
    pub fn verify(&self, limits: KineticZigzagArtifactLimits) -> Result<()> {
        let trajectory = KineticFiltration::new(
            self.vertex_count,
            self.edges.clone(),
            self.start,
            self.end,
            limits.kinetic,
        )?;
        let (rebuilt, _) = Self::build(
            &trajectory,
            self.dimension,
            self.scale,
            self.modulus,
            limits,
        )?;
        if rebuilt != *self {
            return Err(Error::InvalidInput(
                "kinetic zigzag differs from exact replay".into(),
            ));
        }
        Ok(())
    }

    /// Number of graph vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Target cohomology dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Fixed filtration scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Claimed interval-isotypic spaces.
    pub fn intervals(&self) -> &[KineticZigzagIntervalClaim] {
        &self.intervals
    }

    /// Structural size of this artifact claim.
    pub fn summary(&self) -> KineticZigzagArtifactSummary {
        KineticZigzagArtifactSummary {
            edges: self.edges.len(),
            nodes: self.node_ranks.len(),
            arrows: self.arrow_ranks.len(),
            intervals: self.intervals.len(),
            interval_copies: self.intervals.iter().map(|item| item.multiplicity).sum(),
        }
    }
}
