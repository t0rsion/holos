use std::collections::BTreeMap;

/// Resource limits for fixed-scale cohomology and relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CohomologyLimits {
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted simplex count in one dimension.
    pub max_simplices_per_dimension: usize,
    /// Largest total incidence count used by one space.
    pub max_boundary_terms: usize,
    /// Largest accepted cohomology dimension.
    pub max_dimension: usize,
}

impl Default for CohomologyLimits {
    fn default() -> Self {
        Self {
            max_vertices: 1_000_000,
            max_simplices_per_dimension: 20_000_000,
            max_boundary_terms: 200_000_000,
            max_dimension: 8,
        }
    }
}

/// Content identifier of one fixed-scale cohomology space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CohomologySpaceId(pub(crate) [u8; 32]);

/// Identifier of one vector in a canonical cohomology basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CohomologyClassId(pub(crate) [u8; 32]);

/// One nonzero coefficient on an oriented simplex.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CochainTerm {
    /// Simplex vertices in ascending order.
    pub simplex: Vec<usize>,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One vector in a canonical fixed-scale cohomology basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyClass {
    /// Content identifier of this basis vector.
    pub id: CohomologyClassId,
    /// Position in the canonical basis.
    pub basis_index: usize,
    /// Canonical cocycle representative.
    pub terms: Vec<CochainTerm>,
}

/// One nonzero coefficient in a canonical cohomology subspace generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CohomologySubspaceTerm {
    /// Canonical basis class named by this coefficient.
    pub class: CohomologyClassId,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One row in the canonical reduced basis of a cohomology subspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologySubspaceGenerator {
    /// Nonzero terms in canonical class order.
    pub terms: Vec<CohomologySubspaceTerm>,
}

/// A basis-independent subspace of one canonical cohomology space.
///
/// The stored generators are reduced coordinates in the ambient canonical
/// basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologySubspace {
    pub(crate) space: CohomologySpaceId,
    pub(crate) ambient_rank: usize,
    pub(crate) modulus: u32,
    pub(crate) generators: Vec<CohomologySubspaceGenerator>,
}

/// Canonical basis of `H^dimension` at one filtration scale.
#[derive(Debug, Clone)]
pub struct CohomologySpace {
    pub(crate) id: CohomologySpaceId,
    pub(crate) vertex_count: usize,
    pub(crate) dimension: usize,
    pub(crate) scale: f64,
    pub(crate) modulus: u32,
    pub(crate) active_graph_digest: [u8; 32],
    pub(crate) simplex_counts: Vec<usize>,
    pub(crate) simplices: Vec<Vec<usize>>,
    pub(crate) coboundaries: Vec<SparseVector>,
    pub(crate) basis_vectors: Vec<SparseVector>,
    pub(crate) basis: Vec<CohomologyClass>,
}

/// One coefficient on a canonical basis class in a relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CohomologyRelationTerm {
    /// Basis class named by this coefficient.
    pub class: CohomologyClassId,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Equality of old and new class combinations on the common subcomplex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyRelationVector {
    /// Nonzero coefficients on the old canonical basis.
    pub old: Vec<CohomologyRelationTerm>,
    /// Nonzero coefficients on the new canonical basis.
    pub new: Vec<CohomologyRelationTerm>,
}

/// Exact relation between two fixed-scale cohomology spaces.
#[derive(Debug, Clone, PartialEq)]
pub struct CohomologyRelation {
    /// Old space identifier.
    pub old_space: CohomologySpaceId,
    /// New space identifier.
    pub new_space: CohomologySpaceId,
    /// Cohomology dimension.
    pub dimension: usize,
    /// Shared filtration scale.
    pub scale: f64,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Old space rank.
    pub old_rank: usize,
    /// New space rank.
    pub new_rank: usize,
    /// Rank of the old restriction image.
    pub old_image_rank: usize,
    /// Rank of the new restriction image.
    pub new_image_rank: usize,
    /// Dimension of the old restriction kernel.
    pub old_kernel_rank: usize,
    /// Dimension of the new restriction kernel.
    pub new_kernel_rank: usize,
    /// Dimension of the complete relation.
    pub relation_rank: usize,
    /// Canonical basis for the relation.
    pub basis: Vec<CohomologyRelationVector>,
}

/// Outcome of continuing one exact class through a common subcomplex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CohomologyContinuationKind {
    /// The new restriction map has no kernel and one nonzero class matches.
    Unique,
    /// The new restriction map has a nonzero kernel.
    Ambiguous,
    /// No class in the new space has the same restriction.
    NoExtension,
    /// Zero is the only class with the same restriction.
    NoNonzeroContinuation,
}

/// Conservative continuation of one fixed-scale cohomology class.
///
/// For an ambiguous result, `new` is one solution and `ambiguity` is a basis
/// for the additive fiber direction. Every solution is `new` plus a linear
/// combination of `ambiguity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyContinuation {
    /// Old space identifier.
    pub old_space: CohomologySpaceId,
    /// New space identifier.
    pub new_space: CohomologySpaceId,
    /// Classification of the exact affine fiber.
    pub kind: CohomologyContinuationKind,
    /// Selected old class in canonical basis coordinates.
    pub old: Vec<CohomologyRelationTerm>,
    /// Unique target or one base point of an ambiguous fiber.
    pub new: Vec<CohomologyRelationTerm>,
    /// Canonical basis for the ambiguous fiber direction.
    pub ambiguity: Vec<Vec<CohomologyRelationTerm>>,
}

/// One nonzero target coefficient in a cohomology restriction map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CohomologyMapTerm {
    /// Target basis class.
    pub class: CohomologyClassId,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Image of one source basis class under a cohomology restriction map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyMapColumn {
    /// Source basis class.
    pub source: CohomologyClassId,
    /// Nonzero coefficients on the target basis.
    pub image: Vec<CohomologyMapTerm>,
}

/// Exact cohomology map induced by inclusion of an active subcomplex.
#[derive(Debug, Clone, PartialEq)]
pub struct CohomologyRestriction {
    /// Space on the containing active complex.
    pub source_space: CohomologySpaceId,
    /// Space on the active subcomplex.
    pub target_space: CohomologySpaceId,
    /// Cohomology dimension.
    pub dimension: usize,
    /// Shared filtration scale.
    pub scale: f64,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Rank of the restriction map.
    pub rank: usize,
    /// Map columns in source basis order.
    pub columns: Vec<CohomologyMapColumn>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SparseVector(pub(crate) BTreeMap<usize, u32>);
