use crate::ProofLimits;
use crate::circular::CircularProofLimits;

/// Limits for independent bipersistence replay.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BipersistenceProofLimits {
    /// General graph, simplex, and coefficient limits.
    pub proof: ProofLimits,
    /// Largest scale-axis count.
    pub max_scales: usize,
    /// Largest density-axis count.
    pub max_density_levels: usize,
    /// Largest stored rectangle-query count.
    pub max_rectangles: usize,
    /// Largest stored connected-region query count.
    pub max_regions: usize,
    /// Largest stored class-atlas count.
    pub max_class_atlases: usize,
    /// Largest direct-sum dimension used by one rectangle query.
    pub max_linear_variables: usize,
    /// Largest sparse coefficient count used by one rectangle query.
    pub max_linear_terms: usize,
    /// Limits for each nested circular-coordinate proof.
    pub circular: CircularProofLimits,
    /// Largest circular-family count.
    pub max_circular_families: usize,
    /// Largest producer iteration bound carried by a circular family.
    pub max_circular_iterations: usize,
}

impl Default for BipersistenceProofLimits {
    fn default() -> Self {
        Self {
            proof: ProofLimits::default(),
            max_scales: 100_000,
            max_density_levels: 1_000_000,
            max_rectangles: 1_000_000,
            max_regions: 1_000_000,
            max_class_atlases: 1_000_000,
            max_linear_variables: 100_000,
            max_linear_terms: 100_000_000,
            circular: CircularProofLimits::default(),
            max_circular_families: 1_000_000,
            max_circular_iterations: 10_000_000,
        }
    }
}

/// Counts from an independently checked finite bipersistence artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedBipersistence {
    /// Source graph vertex count.
    pub vertices: usize,
    /// Source graph edge count.
    pub edges: usize,
    /// Scale-axis count.
    pub scales: usize,
    /// Density-axis count.
    pub density_levels: usize,
    /// Finite module node count.
    pub nodes: usize,
    /// Exact cover-map count.
    pub cover_maps: usize,
    /// Checked rectangle-rank count.
    pub rectangles: usize,
    /// Checked connected-region rank count.
    pub regions: usize,
    /// Checked class-atlas count.
    pub class_atlases: usize,
    /// Checked circular-family count.
    pub circular_families: usize,
    /// Successful circular-family entries with independently checked coordinates.
    pub circular_family_successes: usize,
    /// Circular-family entries whose bounded lift computation failed.
    ///
    /// This is a computational annotation. It does not prove that an
    /// integral lift is impossible.
    pub circular_family_lift_failures: usize,
    /// Circular-family entries whose bounded harmonic solve failed.
    ///
    /// This is a computational annotation, not a mathematical obstruction.
    pub circular_family_solve_failures: usize,
    /// Circular-family entries where computation was not attempted because
    /// the topology was ambiguous or absent.
    pub circular_family_not_attempted: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Grade {
    pub(crate) scale: usize,
    pub(crate) density: usize,
}

impl Grade {
    pub(crate) fn precedes(self, other: Self) -> bool {
        self.scale <= other.scale && self.density <= other.density
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WeightedEdge {
    pub(crate) u: usize,
    pub(crate) v: usize,
    pub(crate) value_bits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeClaim {
    pub(crate) grade: Grade,
    pub(crate) space: [u8; 32],
    pub(crate) rank: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Term {
    pub(crate) basis: usize,
    pub(crate) coefficient: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) source: usize,
    pub(crate) image: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MapClaim {
    pub(crate) lower: Grade,
    pub(crate) upper: Grade,
    pub(crate) source_space: [u8; 32],
    pub(crate) target_space: [u8; 32],
    pub(crate) rank: usize,
    pub(crate) columns: Vec<Column>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RectangleClaim {
    pub(crate) lower: Grade,
    pub(crate) upper: Grade,
    pub(crate) rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RankRegionClaim {
    pub(crate) grades: Vec<Grade>,
    pub(crate) rank: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExtensionKind {
    Unique,
    Ambiguous,
    NoExtension,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extension {
    pub(crate) grade: Grade,
    pub(crate) kind: ExtensionKind,
    pub(crate) class: Vec<Term>,
    pub(crate) ambiguity: Vec<Vec<Term>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) index: usize,
    pub(crate) kind: ExtensionKind,
    pub(crate) ambiguity_rank: usize,
    pub(crate) grades: Vec<Grade>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AtlasClaim {
    pub(crate) base_grade: Grade,
    pub(crate) base_class: Vec<Term>,
    pub(crate) extensions: Vec<Extension>,
    pub(crate) regions: Vec<Region>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircularEntryClaim {
    pub(crate) grade: Grade,
    pub(crate) extension: ExtensionKind,
    pub(crate) status: CircularFamilyStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Computation status recorded for one topology extension.
pub(crate) enum CircularFamilyStatus {
    /// No computation was attempted for an ambiguous or absent extension.
    NotAttempted,
    /// The bounded producer lift did not produce a checked lift.
    ///
    /// This status does not prove that an integral lift is impossible.
    LiftFailed,
    /// The bounded producer harmonic solve did not finish with a checked
    /// coordinate.
    ///
    /// This status is a computational annotation, not a mathematical
    /// obstruction.
    SolveFailed,
    /// A nested coordinate proof is present and checked independently.
    Success(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CircularFamilyClaim {
    pub(crate) base_grade: Grade,
    pub(crate) base_class: Vec<Term>,
    pub(crate) tolerance_bits: u64,
    /// Bounded producer solver parameter. `HOLOSCC` version 1 does not carry
    /// the producer's iteration count, so the checker does not bind this
    /// parameter to nested coordinate work.
    pub(crate) max_iterations: usize,
    pub(crate) entries: Vec<CircularEntryClaim>,
}

pub(crate) struct Claim {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<WeightedEdge>,
    pub(crate) threshold_bits: u64,
    pub(crate) modulus: u32,
    pub(crate) scale_bits: Vec<u64>,
    pub(crate) minimum_degrees: Vec<usize>,
    pub(crate) nodes: Vec<NodeClaim>,
    pub(crate) cover_maps: Vec<MapClaim>,
    pub(crate) rectangles: Vec<RectangleClaim>,
    pub(crate) rank_regions: Vec<RankRegionClaim>,
    pub(crate) class_atlases: Vec<AtlasClaim>,
    pub(crate) circular_families: Vec<CircularFamilyClaim>,
}
