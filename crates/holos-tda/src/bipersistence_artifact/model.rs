use crate::{
    BifiltrationLimits, Bigrade, BipersistenceLimits, BipersistenceMap, BipersistenceNode,
    BipersistenceRectangle, BipersistenceRegion, BipersistenceTerm, ClassExtensionKind,
    CohomologyClassAtlas,
};

/// Resource limits for one bipersistence artifact and exact replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BipersistenceArtifactLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Largest source graph edge count.
    pub max_source_edges: usize,
    /// Largest stored rectangle-query count.
    pub max_rectangles: usize,
    /// Largest stored connected-region query count.
    pub max_regions: usize,
    /// Largest stored class-atlas count.
    pub max_class_atlases: usize,
    /// Largest stored circular-family count.
    pub max_circular_families: usize,
    /// Largest producer iteration bound carried by a circular family.
    pub max_circular_iterations: usize,
    /// Largest nested `HOLOSCC` coordinate byte count.
    pub max_coordinate_bytes: usize,
    /// Limits for degree-Rips reconstruction.
    pub bifiltration: BifiltrationLimits,
    /// Limits for finite-module reconstruction and exact queries.
    pub module: BipersistenceLimits,
}

impl Default for BipersistenceArtifactLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_source_edges: 200_000_000,
            max_rectangles: 1_000_000,
            max_regions: 1_000_000,
            max_class_atlases: 1_000_000,
            max_circular_families: 1_000_000,
            max_circular_iterations: 10_000_000,
            max_coordinate_bytes: 1 << 30,
            bifiltration: BifiltrationLimits::default(),
            module: BipersistenceLimits::default(),
        }
    }
}

/// One checked generalized rectangle-rank claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BipersistenceRectangleClaim {
    /// Queried closed rectangle.
    pub rectangle: BipersistenceRectangle,
    /// Rank of the limit-to-colimit map.
    pub rank: usize,
}

/// One checked generalized connected-region rank claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BipersistenceRegionClaim {
    /// Queried connected region.
    pub region: BipersistenceRegion,
    /// Rank of the limit-to-colimit map.
    pub rank: usize,
}

/// Structural counts for one bipersistence artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BipersistenceArtifactSummary {
    /// Source graph vertex count.
    pub vertices: usize,
    /// Source graph edge count.
    pub edges: usize,
    /// Finite parameter-grid node count.
    pub nodes: usize,
    /// Stored cover-map count.
    pub cover_maps: usize,
    /// Stored rectangle-rank count.
    pub rectangles: usize,
    /// Stored connected-region rank count.
    pub regions: usize,
    /// Stored class-atlas count.
    pub class_atlases: usize,
    /// Stored checked circular-family count.
    pub circular_families: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ArtifactEdge {
    pub(super) u: usize,
    pub(super) v: usize,
    pub(super) value_bits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ArtifactCircularEntry {
    pub(super) grade: Bigrade,
    pub(super) extension: ClassExtensionKind,
    pub(super) status: ArtifactCircularStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ArtifactCircularStatus {
    NotAttempted,
    LiftFailed,
    SolveFailed,
    Success(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ArtifactCircularFamily {
    pub(super) base_grade: Bigrade,
    pub(super) base_class: Vec<BipersistenceTerm>,
    pub(super) tolerance_bits: u64,
    pub(super) max_iterations: usize,
    pub(super) entries: Vec<ArtifactCircularEntry>,
}

/// Self-contained `HOLOSBP` finite bipersistence artifact.
///
/// The artifact binds the weighted source graph, the degree-Rips parameters,
/// every canonical `H¹` node, every cover map, and selected derived claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BipersistenceArtifact {
    pub(super) vertex_count: usize,
    pub(super) edges: Vec<ArtifactEdge>,
    pub(super) threshold_bits: u64,
    pub(super) modulus: u32,
    pub(super) scale_bits: Vec<u64>,
    pub(super) minimum_degrees: Vec<usize>,
    pub(super) nodes: Vec<BipersistenceNode>,
    pub(super) cover_maps: Vec<BipersistenceMap>,
    pub(super) rectangles: Vec<BipersistenceRectangleClaim>,
    pub(super) regions: Vec<BipersistenceRegionClaim>,
    pub(super) class_atlases: Vec<CohomologyClassAtlas>,
    pub(super) circular_families: Vec<ArtifactCircularFamily>,
    pub(super) digest: [u8; 32],
}
