//! Canonical cohomology spaces and exact relations in any bounded dimension.
//!
//! A space is the quotient of cocycles by coboundaries at one filtration
//! scale. Sparse row reduction gives a deterministic basis on labeled flag
//! simplices. Two spaces relate by restriction to the common active flag
//! subcomplex.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result, SparseDistanceMatrix};

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
pub struct CohomologySpaceId([u8; 32]);

impl CohomologySpaceId {
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for CohomologySpaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

/// Identifier of one vector in a canonical cohomology basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CohomologyClassId([u8; 32]);

impl CohomologyClassId {
    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for CohomologyClassId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

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
/// basis. Changing a generating family does not change the constructed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologySubspace {
    space: CohomologySpaceId,
    ambient_rank: usize,
    modulus: u32,
    generators: Vec<CohomologySubspaceGenerator>,
}

impl CohomologySubspace {
    /// Ambient cohomology space identifier.
    pub fn space(&self) -> CohomologySpaceId {
        self.space
    }

    /// Dimension of the ambient cohomology space.
    pub fn ambient_rank(&self) -> usize {
        self.ambient_rank
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Dimension of this subspace.
    pub fn rank(&self) -> usize {
        self.generators.len()
    }

    /// Canonical reduced generating rows.
    pub fn generators(&self) -> &[CohomologySubspaceGenerator] {
        &self.generators
    }
}

/// Canonical basis of `H^dimension` at one filtration scale.
#[derive(Debug, Clone)]
pub struct CohomologySpace {
    id: CohomologySpaceId,
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    active_graph_digest: [u8; 32],
    simplex_counts: Vec<usize>,
    simplices: Vec<Vec<usize>>,
    coboundaries: Vec<SparseVector>,
    basis_vectors: Vec<SparseVector>,
    basis: Vec<CohomologyClass>,
}

impl CohomologySpace {
    /// Content identifier of the active complex and canonical basis.
    pub fn id(&self) -> CohomologySpaceId {
        self.id
    }

    /// Number of graph vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Cohomology dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Filtration scale of the active flag complex.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Simplex counts from dimension zero through `dimension + 1`.
    pub fn simplex_counts(&self) -> &[usize] {
        &self.simplex_counts
    }

    /// Canonical basis in ascending pivot order.
    pub fn basis(&self) -> &[CohomologyClass] {
        &self.basis
    }

    /// Dimension of this cohomology space.
    pub fn rank(&self) -> usize {
        self.basis.len()
    }

    /// Construct a subspace from coordinate rows in this canonical basis.
    ///
    /// Each pair is `(basis_index, coefficient)`. Repeated positions are not
    /// accepted. Zero rows and dependent rows are removed.
    pub fn subspace_from_coordinates(
        &self,
        rows: &[Vec<(usize, u32)>],
    ) -> Result<CohomologySubspace> {
        let modulus = self.modulus as u64;
        let mut vectors = Vec::with_capacity(rows.len());
        for row in rows {
            if row.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                || row.iter().any(|(position, coefficient)| {
                    *position >= self.rank() || *coefficient == 0 || *coefficient >= self.modulus
                })
            {
                return Err(Error::InvalidInput(
                    "cohomology subspace coordinates are not canonical".into(),
                ));
            }
            let mut vector = SparseVector::default();
            for &(position, coefficient) in row {
                vector.insert(position, coefficient);
            }
            vectors.push(vector);
        }
        let generators = rref(vectors, modulus)
            .into_iter()
            .map(|row| CohomologySubspaceGenerator {
                terms: row
                    .0
                    .into_iter()
                    .map(|(position, coefficient)| CohomologySubspaceTerm {
                        class: self.basis[position].id,
                        coefficient,
                    })
                    .collect(),
            })
            .collect();
        Ok(CohomologySubspace {
            space: self.id,
            ambient_rank: self.rank(),
            modulus: self.modulus,
            generators,
        })
    }

    /// Construct the complete cohomology space as a subspace of itself.
    pub fn full_subspace(&self) -> CohomologySubspace {
        let rows = (0..self.rank())
            .map(|position| vec![(position, 1)])
            .collect::<Vec<_>>();
        self.subspace_from_coordinates(&rows)
            .expect("canonical basis coordinates are valid")
    }

    /// Return canonical coordinate rows for a subspace of this space.
    pub fn subspace_coordinates(
        &self,
        subspace: &CohomologySubspace,
    ) -> Result<Vec<Vec<(usize, u32)>>> {
        if subspace.space != self.id
            || subspace.modulus != self.modulus
            || subspace.ambient_rank != self.rank()
        {
            return Err(Error::InvalidInput(
                "cohomology subspace belongs to a different ambient space".into(),
            ));
        }
        let positions = self
            .basis
            .iter()
            .enumerate()
            .map(|(position, class)| (class.id, position))
            .collect::<BTreeMap<_, _>>();
        subspace
            .generators
            .iter()
            .map(|generator| {
                generator
                    .terms
                    .iter()
                    .map(|term| {
                        positions
                            .get(&term.class)
                            .copied()
                            .map(|position| (position, term.coefficient))
                            .ok_or_else(|| {
                                Error::InvalidInput(
                                    "cohomology subspace names an unknown class".into(),
                                )
                            })
                    })
                    .collect()
            })
            .collect()
    }

    fn require_graph(&self, graph: &SparseDistanceMatrix) -> Result<()> {
        if graph.len() != self.vertex_count
            || active_graph_digest(graph, self.scale) != self.active_graph_digest
        {
            return Err(Error::InvalidInput(
                "cohomology space is bound to a different active graph".into(),
            ));
        }
        Ok(())
    }
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
    /// Dimension of the exact image intersection.
    pub relation_rank: usize,
    /// Canonical basis for the relation.
    pub basis: Vec<CohomologyRelationVector>,
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

impl CohomologyRestriction {
    /// Whether the restriction image contains one target basis class.
    pub fn image_contains(&self, class: CohomologyClassId) -> bool {
        let classes = self
            .columns
            .iter()
            .flat_map(|column| column.image.iter().map(|term| term.class))
            .chain(std::iter::once(class))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let positions = classes
            .iter()
            .copied()
            .enumerate()
            .map(|(position, id)| (id, position))
            .collect::<BTreeMap<_, _>>();
        let rows = self
            .columns
            .iter()
            .map(|column| {
                let mut row = SparseVector::default();
                for term in &column.image {
                    row.insert(positions[&term.class], term.coefficient);
                }
                row
            })
            .collect();
        let mut target = SparseVector::default();
        target.insert(positions[&class], 1);
        reduce_by_basis(
            &mut target,
            &rref(rows, self.modulus as u64),
            self.modulus as u64,
        );
        target.is_zero()
    }

    /// Dimension of the intersection between the image and a target subspace.
    pub fn image_intersection_rank(
        &self,
        target: &CohomologySpace,
        subspace: &CohomologySubspace,
    ) -> Result<usize> {
        if self.target_space != target.id
            || subspace.space != target.id
            || self.modulus != target.modulus
            || subspace.modulus != target.modulus
            || subspace.ambient_rank != target.rank()
        {
            return Err(Error::InvalidInput(
                "cohomology subspace belongs to a different restriction target".into(),
            ));
        }
        let positions = target
            .basis
            .iter()
            .enumerate()
            .map(|(position, class)| (class.id, position))
            .collect::<BTreeMap<_, _>>();
        let image = self
            .columns
            .iter()
            .map(|column| {
                let mut row = SparseVector::default();
                for term in &column.image {
                    row.insert(positions[&term.class], term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let subspace_rows = subspace
            .generators
            .iter()
            .map(|generator| {
                let mut row = SparseVector::default();
                for term in &generator.terms {
                    row.insert(positions[&term.class], term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let modulus = self.modulus as u64;
        let image_rank = rref(image.clone(), modulus).len();
        let subspace_rank = subspace_rows.len();
        let union_rank = rref(image.into_iter().chain(subspace_rows).collect(), modulus).len();
        image_rank
            .checked_add(subspace_rank)
            .and_then(|sum| sum.checked_sub(union_rank))
            .ok_or_else(|| Error::InvalidInput("cohomology intersection rank is invalid".into()))
    }
}

impl CohomologyRelation {
    /// True when the relation is an isomorphism of both spaces.
    pub fn is_isomorphism(&self) -> bool {
        self.relation_rank == self.old_rank
            && self.relation_rank == self.new_rank
            && self.old_image_rank == self.old_rank
            && self.new_image_rank == self.new_rank
    }

    /// Whether the old basis class occurs in the relation projection.
    pub fn contains_old_class(&self, class: CohomologyClassId) -> bool {
        let classes: Vec<_> = self
            .basis
            .iter()
            .flat_map(|vector| vector.old.iter().map(|term| term.class))
            .chain(std::iter::once(class))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let positions: BTreeMap<_, _> = classes
            .iter()
            .copied()
            .enumerate()
            .map(|(position, id)| (id, position))
            .collect();
        let rows: Vec<_> = self
            .basis
            .iter()
            .map(|vector| relation_row(&vector.old, &positions))
            .collect();
        let mut target = SparseVector::default();
        if let Some(&position) = positions.get(&class) {
            target.insert(position, 1);
        }
        reduce_by_basis(
            &mut target,
            &rref(rows, self.modulus as u64),
            self.modulus as u64,
        );
        target.is_zero()
    }
}

/// Compute a canonical cohomology space on the active flag complex.
pub fn cohomology_space(
    graph: &SparseDistanceMatrix,
    dimension: usize,
    scale: f64,
    modulus: u32,
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    validate(graph.len(), dimension, scale, modulus, limits)?;
    let complex = ActiveComplex::build(
        graph.len(),
        dimension,
        |u, v| graph.get(u, v) <= scale,
        limits,
    )?;
    space_from_complex(
        complex,
        graph.len(),
        dimension,
        scale,
        modulus,
        active_graph_digest(graph, scale),
        limits,
    )
}

/// Relate two canonical spaces by restriction to their common active flag subcomplex.
///
/// Restriction gives maps from both full cohomology spaces to the common
/// space. The returned basis is their exact image intersection. An empty
/// relation does not prove that either full-space class is absent.
pub fn cohomology_relation(
    old_graph: &SparseDistanceMatrix,
    old: &CohomologySpace,
    new_graph: &SparseDistanceMatrix,
    new: &CohomologySpace,
    limits: CohomologyLimits,
) -> Result<CohomologyRelation> {
    old.require_graph(old_graph)?;
    new.require_graph(new_graph)?;
    if old.vertex_count != new.vertex_count
        || old.dimension != new.dimension
        || old.modulus != new.modulus
        || old.scale.to_bits() != new.scale.to_bits()
    {
        return Err(Error::InvalidInput(
            "cohomology relation requires one vertex set, dimension, scale, and field".into(),
        ));
    }
    let common = ActiveComplex::build(
        old.vertex_count,
        old.dimension,
        |u, v| old_graph.get(u, v) <= old.scale && new_graph.get(u, v) <= old.scale,
        limits,
    )?;
    let common = space_from_complex(
        common,
        old.vertex_count,
        old.dimension,
        old.scale,
        old.modulus,
        common_graph_digest(old_graph, new_graph, old.scale),
        limits,
    )?;
    let old_rows = restricted_rows(old, &common);
    let new_rows = restricted_rows(new, &common);
    let old_image = independent_image(&old_rows, old.modulus as u64);
    let new_image = independent_image(&new_rows, old.modulus as u64);
    let relations = image_intersection(
        &old_image,
        &new_image,
        common.simplices.len(),
        old.modulus as u64,
    );
    let basis = relations
        .into_iter()
        .map(
            |(old_coefficients, new_coefficients)| CohomologyRelationVector {
                old: relation_terms(old.basis(), &old_coefficients),
                new: relation_terms(new.basis(), &new_coefficients),
            },
        )
        .collect::<Vec<_>>();
    Ok(CohomologyRelation {
        old_space: old.id,
        new_space: new.id,
        dimension: old.dimension,
        scale: old.scale,
        modulus: old.modulus,
        old_rank: old.rank(),
        new_rank: new.rank(),
        old_image_rank: old_image.len(),
        new_image_rank: new_image.len(),
        relation_rank: basis.len(),
        basis,
    })
}

/// Compute the cohomology restriction from a complex to an active subcomplex.
///
/// Every active edge of `target_graph` must also be active in `source_graph`.
/// The returned columns use the canonical bases of `source` and `target`.
pub fn cohomology_restriction(
    source_graph: &SparseDistanceMatrix,
    source: &CohomologySpace,
    target_graph: &SparseDistanceMatrix,
    target: &CohomologySpace,
) -> Result<CohomologyRestriction> {
    source.require_graph(source_graph)?;
    target.require_graph(target_graph)?;
    if source.vertex_count != target.vertex_count
        || source.dimension != target.dimension
        || source.modulus != target.modulus
        || source.scale.to_bits() != target.scale.to_bits()
    {
        return Err(Error::InvalidInput(
            "cohomology restriction requires one vertex set, dimension, scale, and field".into(),
        ));
    }
    if target_graph
        .edges()
        .any(|(u, v, value)| value <= target.scale && source_graph.get(u, v) > source.scale)
    {
        return Err(Error::InvalidInput(
            "cohomology restriction target is not an active subcomplex".into(),
        ));
    }
    let modulus = source.modulus as u64;
    let images = restricted_rows(source, target);
    let coordinates = images
        .iter()
        .map(|image| coordinates_in_basis(image, &target.basis_vectors, modulus))
        .collect::<Result<Vec<_>>>()?;
    let rank = rref(coordinates.clone(), modulus).len();
    let columns = source
        .basis
        .iter()
        .zip(coordinates)
        .map(|(class, coordinates)| CohomologyMapColumn {
            source: class.id,
            image: coordinates
                .0
                .into_iter()
                .map(|(position, coefficient)| CohomologyMapTerm {
                    class: target.basis[position].id,
                    coefficient,
                })
                .collect(),
        })
        .collect();
    Ok(CohomologyRestriction {
        source_space: source.id,
        target_space: target.id,
        dimension: source.dimension,
        scale: source.scale,
        modulus: source.modulus,
        rank,
        columns,
    })
}

fn validate(
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    limits: CohomologyLimits,
) -> Result<()> {
    if vertex_count > limits.max_vertices {
        return Err(Error::InvalidInput(format!(
            "cohomology vertex count exceeds the limit {}",
            limits.max_vertices
        )));
    }
    if dimension > limits.max_dimension {
        return Err(Error::InvalidInput(format!(
            "cohomology dimension exceeds the limit {}",
            limits.max_dimension
        )));
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(Error::InvalidInput(
            "cohomology scale must be finite and non-negative".into(),
        ));
    }
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(
            "cohomology modulus must be a supported prime".into(),
        ));
    }
    Ok(())
}

struct ActiveComplex {
    dimensions: Vec<Vec<Vec<usize>>>,
}

impl ActiveComplex {
    fn build<F>(
        vertex_count: usize,
        max_dimension: usize,
        mut active: F,
        limits: CohomologyLimits,
    ) -> Result<Self>
    where
        F: FnMut(usize, usize) -> bool,
    {
        let vertices: Vec<_> = (0..vertex_count).map(|vertex| vec![vertex]).collect();
        if vertices.len() > limits.max_simplices_per_dimension {
            return Err(Error::InvalidInput(
                "cohomology vertex simplex count exceeds its limit".into(),
            ));
        }
        let mut dimensions = vec![vertices];
        for dimension in 1..=max_dimension + 1 {
            let mut next = Vec::new();
            for simplex in &dimensions[dimension - 1] {
                let start = simplex.last().copied().unwrap_or(0) + 1;
                for vertex in start..vertex_count {
                    if simplex.iter().all(|&member| active(member, vertex)) {
                        let mut cofacet = simplex.clone();
                        cofacet.push(vertex);
                        next.push(cofacet);
                        if next.len() > limits.max_simplices_per_dimension {
                            return Err(Error::InvalidInput(format!(
                                "dimension {dimension} cohomology simplex count exceeds the limit {}",
                                limits.max_simplices_per_dimension
                            )));
                        }
                    }
                }
            }
            dimensions.push(next);
        }
        Ok(Self { dimensions })
    }
}

fn space_from_complex(
    complex: ActiveComplex,
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    active_graph_digest: [u8; 32],
    limits: CohomologyLimits,
) -> Result<CohomologySpace> {
    let incidence_count = complex.dimensions[dimension]
        .len()
        .checked_mul(dimension + 1)
        .and_then(|count| {
            complex.dimensions[dimension + 1]
                .len()
                .checked_mul(dimension + 2)
                .and_then(|next| count.checked_add(next))
        })
        .ok_or_else(|| Error::InvalidInput("cohomology incidence count overflows".into()))?;
    if incidence_count > limits.max_boundary_terms {
        return Err(Error::InvalidInput(format!(
            "cohomology incidence count exceeds the limit {}",
            limits.max_boundary_terms
        )));
    }
    let modulus64 = modulus as u64;
    let q_simplices = &complex.dimensions[dimension];
    let q_positions: BTreeMap<_, _> = q_simplices
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let cocycle_equations =
        coboundary_equations(&complex.dimensions[dimension + 1], &q_positions, modulus64)?;
    let cocycles = nullspace(cocycle_equations, q_simplices.len(), modulus64);
    let coboundaries = if dimension == 0 {
        Vec::new()
    } else {
        coboundary_image(&complex.dimensions[dimension - 1], q_simplices, modulus64)?
    };
    let coboundaries = rref(coboundaries, modulus64);
    let mut quotient = Vec::new();
    for mut cocycle in cocycles {
        reduce_by_basis(&mut cocycle, &coboundaries, modulus64);
        if !cocycle.is_zero() {
            quotient.push(cocycle);
        }
    }
    let basis_vectors = rref(quotient, modulus64);
    let simplex_counts = complex.dimensions.iter().map(Vec::len).collect::<Vec<_>>();
    let id = space_id(
        vertex_count,
        dimension,
        scale,
        modulus,
        &active_graph_digest,
        q_simplices,
        &basis_vectors,
    );
    let basis = basis_vectors
        .iter()
        .enumerate()
        .map(|(basis_index, vector)| CohomologyClass {
            id: class_id(id, basis_index, vector),
            basis_index,
            terms: vector
                .0
                .iter()
                .map(|(&position, &coefficient)| CochainTerm {
                    simplex: q_simplices[position].clone(),
                    coefficient,
                })
                .collect(),
        })
        .collect();
    Ok(CohomologySpace {
        id,
        vertex_count,
        dimension,
        scale,
        modulus,
        active_graph_digest,
        simplex_counts,
        simplices: q_simplices.clone(),
        coboundaries,
        basis_vectors,
        basis,
    })
}

fn coboundary_equations(
    cofacets: &[Vec<usize>],
    face_positions: &BTreeMap<Vec<usize>, usize>,
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    cofacets
        .iter()
        .map(|cofacet| {
            let mut equation = SparseVector::default();
            for removed in 0..cofacet.len() {
                let mut face = cofacet.clone();
                face.remove(removed);
                let position = face_positions.get(&face).copied().ok_or_else(|| {
                    Error::InvalidInput("cohomology coboundary omits a face".into())
                })?;
                equation.insert(position, sign(removed, modulus));
            }
            Ok(equation)
        })
        .collect()
}

fn coboundary_image(
    faces: &[Vec<usize>],
    simplices: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    let face_positions: BTreeMap<_, _> = faces
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let mut generators = vec![SparseVector::default(); faces.len()];
    for (simplex_position, simplex) in simplices.iter().enumerate() {
        for removed in 0..simplex.len() {
            let mut face = simplex.clone();
            face.remove(removed);
            let position = face_positions
                .get(&face)
                .copied()
                .ok_or_else(|| Error::InvalidInput("cohomology boundary omits a face".into()))?;
            generators[position].insert(simplex_position, sign(removed, modulus));
        }
    }
    Ok(generators)
}

fn sign(position: usize, modulus: u64) -> u32 {
    if position % 2 == 0 {
        1
    } else {
        (modulus - 1) as u32
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SparseVector(BTreeMap<usize, u32>);

impl SparseVector {
    fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    fn leading(&self) -> Option<(usize, u32)> {
        self.0.first_key_value().map(|(&key, &value)| (key, value))
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&position, &coefficient) in &source.0 {
            let current = u64::from(self.0.get(&position).copied().unwrap_or(0));
            let next = (current + factor * u64::from(coefficient)) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next as u32);
            }
        }
    }

    fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
}

fn rref(rows: Vec<SparseVector>, modulus: u64) -> Vec<SparseVector> {
    let mut basis: Vec<SparseVector> = Vec::new();
    for mut row in rows {
        reduce_by_basis(&mut row, &basis, modulus);
        let Some((pivot, coefficient)) = row.leading() else {
            continue;
        };
        row.scale(inverse_mod(u64::from(coefficient), modulus), modulus);
        for existing in &mut basis {
            if let Some(&value) = existing.0.get(&pivot) {
                existing.add_scaled(&row, modulus - u64::from(value), modulus);
            }
        }
        basis.push(row);
        basis.sort_by_key(|item| item.leading().map(|(position, _)| position));
    }
    basis
}

fn reduce_by_basis(row: &mut SparseVector, basis: &[SparseVector], modulus: u64) {
    for existing in basis {
        let Some((pivot, _)) = existing.leading() else {
            continue;
        };
        if let Some(&coefficient) = row.0.get(&pivot) {
            row.add_scaled(existing, modulus - u64::from(coefficient), modulus);
        }
    }
}

fn nullspace(equations: Vec<SparseVector>, variables: usize, modulus: u64) -> Vec<SparseVector> {
    let equations = rref(equations, modulus);
    let pivots: BTreeSet<_> = equations
        .iter()
        .filter_map(|row| row.leading().map(|(position, _)| position))
        .collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|position| !pivots.contains(position)) {
        let mut vector = SparseVector::default();
        vector.insert(free, 1);
        for equation in &equations {
            let pivot = equation.leading().unwrap().0;
            if let Some(&coefficient) = equation.0.get(&free) {
                vector.insert(pivot, (modulus - u64::from(coefficient)) as u32);
            }
        }
        basis.push(vector);
    }
    basis
}

fn restricted_rows(source: &CohomologySpace, common: &CohomologySpace) -> Vec<SparseVector> {
    let positions: BTreeMap<_, _> = common
        .simplices
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    source
        .basis_vectors
        .iter()
        .map(|vector| {
            let mut restricted = SparseVector::default();
            for (&position, &coefficient) in &vector.0 {
                if let Some(&common_position) = positions.get(&source.simplices[position]) {
                    restricted.insert(common_position, coefficient);
                }
            }
            reduce_by_basis(&mut restricted, &common.coboundaries, common.modulus as u64);
            restricted
        })
        .collect()
}

fn coordinates_in_basis(
    vector: &SparseVector,
    basis: &[SparseVector],
    modulus: u64,
) -> Result<SparseVector> {
    let mut residual = vector.clone();
    let mut coordinates = SparseVector::default();
    for (position, basis_vector) in basis.iter().enumerate() {
        let pivot = basis_vector
            .leading()
            .expect("a reduced basis does not contain zero")
            .0;
        if let Some(&coefficient) = residual.0.get(&pivot) {
            coordinates.insert(position, coefficient);
            residual.add_scaled(basis_vector, modulus - u64::from(coefficient), modulus);
        }
    }
    if !residual.is_zero() {
        return Err(Error::InvalidInput(
            "restricted cocycle is outside the target cohomology basis".into(),
        ));
    }
    Ok(coordinates)
}

#[derive(Debug, Clone)]
struct ImageVector {
    vector: SparseVector,
    source: SparseVector,
}

fn independent_image(rows: &[SparseVector], modulus: u64) -> Vec<ImageVector> {
    let mut basis: Vec<ImageVector> = Vec::new();
    for (position, source_row) in rows.iter().enumerate() {
        let mut vector = source_row.clone();
        let mut source = SparseVector::default();
        source.insert(position, 1);
        for existing in &basis {
            let pivot = existing.vector.leading().unwrap().0;
            if let Some(&coefficient) = vector.0.get(&pivot) {
                let factor = modulus - u64::from(coefficient);
                vector.add_scaled(&existing.vector, factor, modulus);
                source.add_scaled(&existing.source, factor, modulus);
            }
        }
        let Some((pivot, coefficient)) = vector.leading() else {
            continue;
        };
        let inverse = inverse_mod(u64::from(coefficient), modulus);
        vector.scale(inverse, modulus);
        source.scale(inverse, modulus);
        for existing in &mut basis {
            if let Some(&coefficient) = existing.vector.0.get(&pivot) {
                let factor = modulus - u64::from(coefficient);
                existing.vector.add_scaled(&vector, factor, modulus);
                existing.source.add_scaled(&source, factor, modulus);
            }
        }
        basis.push(ImageVector { vector, source });
        basis.sort_by_key(|item| item.vector.leading().map(|(position, _)| position));
    }
    basis
}

fn image_intersection(
    old: &[ImageVector],
    new: &[ImageVector],
    coordinates: usize,
    modulus: u64,
) -> Vec<(SparseVector, SparseVector)> {
    if old.is_empty() || new.is_empty() {
        return Vec::new();
    }
    let mut equations = vec![SparseVector::default(); coordinates];
    for (variable, image) in old.iter().enumerate() {
        for (&coordinate, &coefficient) in &image.vector.0 {
            equations[coordinate].insert(variable, coefficient);
        }
    }
    for (offset, image) in new.iter().enumerate() {
        for (&coordinate, &coefficient) in &image.vector.0 {
            equations[coordinate].insert(
                old.len() + offset,
                (modulus - u64::from(coefficient)) as u32,
            );
        }
    }
    nullspace(equations, old.len() + new.len(), modulus)
        .into_iter()
        .filter_map(|relation| {
            let mut old_source = SparseVector::default();
            for (&position, &coefficient) in relation.0.range(..old.len()) {
                old_source.add_scaled(&old[position].source, u64::from(coefficient), modulus);
            }
            let mut new_source = SparseVector::default();
            for (&position, &coefficient) in relation.0.range(old.len()..) {
                new_source.add_scaled(
                    &new[position - old.len()].source,
                    u64::from(coefficient),
                    modulus,
                );
            }
            (!old_source.is_zero() && !new_source.is_zero()).then_some((old_source, new_source))
        })
        .collect()
}

fn relation_terms(
    basis: &[CohomologyClass],
    coefficients: &SparseVector,
) -> Vec<CohomologyRelationTerm> {
    coefficients
        .0
        .iter()
        .map(|(&position, &coefficient)| CohomologyRelationTerm {
            class: basis[position].id,
            coefficient,
        })
        .collect()
}

fn relation_row(
    terms: &[CohomologyRelationTerm],
    positions: &BTreeMap<CohomologyClassId, usize>,
) -> SparseVector {
    let mut row = SparseVector::default();
    for term in terms {
        if let Some(&position) = positions.get(&term.class) {
            row.insert(position, term.coefficient);
        }
    }
    row
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}

fn active_graph_digest(graph: &SparseDistanceMatrix, scale: f64) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-active-flag-graph-v1");
    hash.update((graph.len() as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for (u, v, _) in graph.edges().filter(|edge| edge.2 <= scale) {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
    }
    hash.finalize().into()
}

fn common_graph_digest(
    old: &SparseDistanceMatrix,
    new: &SparseDistanceMatrix,
    scale: f64,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-common-active-flag-graph-v1");
    hash.update((old.len() as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for (u, v, _) in old
        .edges()
        .filter(|&(u, v, value)| value <= scale && new.get(u, v) <= scale)
    {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
    }
    hash.finalize().into()
}

fn space_id(
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    graph_digest: &[u8; 32],
    simplices: &[Vec<usize>],
    basis: &[SparseVector],
) -> CohomologySpaceId {
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-space-v1");
    hash.update((vertex_count as u64).to_be_bytes());
    hash.update((dimension as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    hash.update(modulus.to_be_bytes());
    hash.update(graph_digest);
    hash.update((basis.len() as u64).to_be_bytes());
    for vector in basis {
        hash.update((vector.0.len() as u64).to_be_bytes());
        for (&position, &coefficient) in &vector.0 {
            hash.update((simplices[position].len() as u64).to_be_bytes());
            for vertex in &simplices[position] {
                hash.update((*vertex as u64).to_be_bytes());
            }
            hash.update(coefficient.to_be_bytes());
        }
    }
    CohomologySpaceId(hash.finalize().into())
}

fn class_id(
    space: CohomologySpaceId,
    basis_index: usize,
    vector: &SparseVector,
) -> CohomologyClassId {
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-class-v1");
    hash.update(space.as_bytes());
    hash.update((basis_index as u64).to_be_bytes());
    hash.update((vector.0.len() as u64).to_be_bytes());
    for (&position, &coefficient) in &vector.0 {
        hash.update((position as u64).to_be_bytes());
        hash.update(coefficient.to_be_bytes());
    }
    CohomologyClassId(hash.finalize().into())
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RipsParams, rips_persistence_sparse};

    fn cross_polytope(pairs: usize) -> SparseDistanceMatrix {
        let vertices = 2 * pairs;
        let edges: Vec<_> = (0..vertices)
            .flat_map(|u| ((u + 1)..vertices).map(move |v| (u, v)))
            .filter(|&(u, v)| u / 2 != v / 2)
            .map(|(u, v)| (u, v, 1.0))
            .collect();
        SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap()
    }

    #[test]
    fn ranks_match_active_bars_through_h3_over_prime_fields() {
        let cycle = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let cases = [(cycle, 1), (cross_polytope(3), 2), (cross_polytope(4), 3)];
        for (graph, dimension) in cases {
            for modulus in [2, 3, 5] {
                let space =
                    cohomology_space(&graph, dimension, 1.0, modulus, CohomologyLimits::default())
                        .unwrap();
                let diagram = rips_persistence_sparse(
                    &graph,
                    &RipsParams::new(dimension).with_modulus(modulus),
                )
                .unwrap();
                let active = diagram
                    .in_dim(dimension)
                    .filter(|bar| bar.birth <= 1.0 && 1.0 < bar.death)
                    .count();
                assert_eq!(space.rank(), active);
                assert_eq!(space.rank(), 1);
            }
        }
    }

    #[test]
    fn h0_basis_counts_components() {
        let graph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
        let space = cohomology_space(&graph, 0, 1.0, 3, CohomologyLimits::default()).unwrap();
        assert_eq!(space.rank(), 2);
    }

    #[test]
    fn restriction_map_uses_canonical_quotient_coordinates() {
        let subgraph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
        let containing =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap();
        let source = cohomology_space(&containing, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
        let target = cohomology_space(&subgraph, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
        let restriction = cohomology_restriction(&containing, &source, &subgraph, &target).unwrap();
        assert_eq!(restriction.rank, 1);
        assert_eq!(restriction.columns.len(), 1);
        assert_eq!(restriction.columns[0].image.len(), 2);

        let identity = cohomology_restriction(&subgraph, &target, &subgraph, &target).unwrap();
        assert_eq!(identity.rank, 2);
        assert_eq!(identity.columns.len(), 2);
    }

    #[test]
    fn identity_is_an_isomorphism_and_a_filled_sphere_dies() {
        let sphere = cross_polytope(3);
        let old = cohomology_space(&sphere, 2, 1.0, 5, CohomologyLimits::default()).unwrap();
        let identity =
            cohomology_relation(&sphere, &old, &sphere, &old, CohomologyLimits::default()).unwrap();
        assert!(identity.is_isomorphism());
        assert!(identity.contains_old_class(old.basis()[0].id));

        let mut edges: Vec<_> = sphere.edges().collect();
        edges.push((0, 1, 1.0));
        let filled = SparseDistanceMatrix::from_triplets(6, &edges).unwrap();
        let new = cohomology_space(&filled, 2, 1.0, 5, CohomologyLimits::default()).unwrap();
        assert_eq!(new.rank(), 0);
        let relation =
            cohomology_relation(&sphere, &old, &filled, &new, CohomologyLimits::default()).unwrap();
        assert_eq!(relation.relation_rank, 0);
        assert!(!relation.contains_old_class(old.basis()[0].id));
    }

    #[test]
    fn subspace_generators_are_canonical_and_intersect_restriction_images() {
        let base = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
        let joined =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap();
        let target = cohomology_space(&base, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
        let source = cohomology_space(&joined, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
        let restriction = cohomology_restriction(&joined, &source, &base, &target).unwrap();
        let full = target.full_subspace();
        assert_eq!(full.rank(), 2);
        assert_eq!(
            restriction.image_intersection_rank(&target, &full).unwrap(),
            1
        );

        let line = target
            .subspace_from_coordinates(&[vec![(0, 2)], vec![(0, 1)]])
            .unwrap();
        assert_eq!(line.rank(), 1);
        assert!(restriction.image_intersection_rank(&target, &line).unwrap() <= 1);
        assert!(
            target
                .subspace_from_coordinates(&[vec![(0, 1), (0, 2)]])
                .is_err()
        );
    }
}
