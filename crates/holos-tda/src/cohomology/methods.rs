use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{Error, Result, SparseDistanceMatrix};

use super::algebra::{
    basis_positions, checked_coordinate_vector, coordinates_in_basis, reduce_by_basis, rref,
};
use super::digest::{active_graph_digest, write_hex};
use super::model::*;

fn checked_class_terms<T, Class, Coefficient>(
    terms: &[T],
    positions: &BTreeMap<CohomologyClassId, usize>,
    modulus: u32,
    message: &str,
    class: Class,
    coefficient: Coefficient,
) -> Result<SparseVector>
where
    Class: Fn(&T) -> CohomologyClassId,
    Coefficient: Fn(&T) -> u32,
{
    let mut row = SparseVector::default();
    let mut prior = None;
    for term in terms {
        let value = coefficient(term);
        if value == 0 || value >= modulus {
            return Err(Error::InvalidInput(message.into()));
        }
        let position = positions
            .get(&class(term))
            .copied()
            .ok_or_else(|| Error::InvalidInput(message.into()))?;
        if prior.is_some_and(|prior| prior >= position) {
            return Err(Error::InvalidInput(message.into()));
        }
        row.insert(position, value);
        prior = Some(position);
    }
    Ok(row)
}

pub(crate) fn checked_relation_row(
    terms: &[CohomologyRelationTerm],
    positions: &BTreeMap<CohomologyClassId, usize>,
    modulus: u32,
    message: &str,
) -> Result<SparseVector> {
    checked_class_terms(
        terms,
        positions,
        modulus,
        message,
        |term| term.class,
        |term| term.coefficient,
    )
}

fn checked_restriction_rows(
    columns: &[CohomologyMapColumn],
    positions: &BTreeMap<CohomologyClassId, usize>,
    modulus: u32,
) -> Result<Vec<SparseVector>> {
    let mut sources = BTreeSet::new();
    columns
        .iter()
        .map(|column| {
            if !sources.insert(column.source) {
                return Err(Error::InvalidInput(
                    "cohomology restriction repeats a source class".into(),
                ));
            }
            checked_class_terms(
                &column.image,
                positions,
                modulus,
                "cohomology restriction image terms are not canonical",
                |term| term.class,
                |term| term.coefficient,
            )
        })
        .collect()
}

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

    /// Resolve a cocycle into this space's canonical quotient coordinates.
    ///
    /// Terms use the orientation and simplex order returned by [`Self::basis`].
    /// The empty result denotes the zero cohomology class.
    pub fn coordinates_of_cocycle(&self, terms: &[CochainTerm]) -> Result<Vec<(usize, u32)>> {
        if terms
            .windows(2)
            .any(|pair| pair[0].simplex >= pair[1].simplex)
            || terms.iter().any(|term| {
                term.simplex.len() != self.dimension + 1
                    || term.coefficient == 0
                    || term.coefficient >= self.modulus
            })
        {
            return Err(Error::InvalidInput(
                "cohomology cocycle terms are not canonical".into(),
            ));
        }
        let positions = self
            .simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect::<BTreeMap<_, _>>();
        let mut vector = SparseVector::default();
        for term in terms {
            let position = positions.get(&term.simplex).copied().ok_or_else(|| {
                Error::InvalidInput("cohomology cocycle uses an inactive simplex".into())
            })?;
            vector.insert(position, term.coefficient);
        }
        reduce_by_basis(&mut vector, &self.coboundaries, self.modulus as u64);
        Ok(
            coordinates_in_basis(&vector, &self.basis_vectors, self.modulus as u64)?
                .0
                .into_iter()
                .collect(),
        )
    }

    /// Construct the canonical cocycle for quotient coordinates.
    ///
    /// Each pair is `(basis_index, coefficient)`. Pairs must use ascending
    /// positions and nonzero field coefficients.
    pub fn cocycle_from_coordinates(
        &self,
        coordinates: &[(usize, u32)],
    ) -> Result<Vec<CochainTerm>> {
        let vector = checked_coordinate_vector(
            coordinates,
            self.rank(),
            self.modulus,
            "cohomology class coordinates are not canonical",
        )?;
        let mut cocycle = SparseVector::default();
        for (&position, &coefficient) in &vector.0 {
            cocycle.add_scaled(
                &self.basis_vectors[position],
                u64::from(coefficient),
                self.modulus as u64,
            );
        }
        Ok(cocycle
            .0
            .into_iter()
            .map(|(position, coefficient)| CochainTerm {
                simplex: self.simplices[position].clone(),
                coefficient,
            })
            .collect())
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
            vectors.push(checked_coordinate_vector(
                row,
                self.rank(),
                self.modulus,
                "cohomology subspace coordinates are not canonical",
            )?);
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
        let positions = basis_positions(self);
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

    pub(crate) fn require_graph(&self, graph: &SparseDistanceMatrix) -> Result<()> {
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

impl CohomologyRestriction {
    /// Whether the restriction image contains one target basis class.
    pub fn image_contains(
        &self,
        target: &CohomologySpace,
        class: CohomologyClassId,
    ) -> Result<bool> {
        let (positions, rows) = self.checked_target_rows(target)?;
        let position = positions.get(&class).copied().ok_or_else(|| {
            Error::InvalidInput("cohomology restriction names an unknown target class".into())
        })?;
        let mut target = SparseVector::default();
        target.insert(position, 1);
        reduce_by_basis(&mut target, &rows, self.modulus as u64);
        Ok(target.is_zero())
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
        let (positions, image) = self.checked_target_rows(target)?;
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
        let image_rank = image.len();
        let subspace_rank = subspace_rows.len();
        let union_rank = rref(image.into_iter().chain(subspace_rows).collect(), modulus).len();
        image_rank
            .checked_add(subspace_rank)
            .and_then(|sum| sum.checked_sub(union_rank))
            .ok_or_else(|| Error::InvalidInput("cohomology intersection rank is invalid".into()))
    }

    fn checked_target_rows(
        &self,
        target: &CohomologySpace,
    ) -> Result<(BTreeMap<CohomologyClassId, usize>, Vec<SparseVector>)> {
        if self.target_space != target.id
            || self.dimension != target.dimension
            || self.scale.to_bits() != target.scale.to_bits()
            || self.modulus != target.modulus
        {
            return Err(Error::InvalidInput(
                "cohomology restriction does not match its target space".into(),
            ));
        }
        let positions = basis_positions(target);
        let rows = rref(
            checked_restriction_rows(&self.columns, &positions, self.modulus)?,
            self.modulus as u64,
        );
        let rank = rows.len();
        if self.rank != rank || self.rank > target.rank() {
            return Err(Error::InvalidInput(
                "cohomology restriction rank is not canonical".into(),
            ));
        }
        Ok((positions, rows))
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

    /// Whether an old basis class occurs in the old projection.
    ///
    /// The check validates the old-space header, old-side rank metadata,
    /// canonical old terms, and the implied old projection rank. It does not
    /// validate new-side terms.
    pub fn contains_old_class(
        &self,
        old: &CohomologySpace,
        class: CohomologyClassId,
    ) -> Result<bool> {
        self.validate_old_projection(old)?;
        let positions = basis_positions(old);
        let rows = self
            .basis
            .iter()
            .map(|vector| {
                if vector.old.is_empty() && vector.new.is_empty() {
                    return Err(Error::InvalidInput(
                        "cohomology relation contains a zero vector".into(),
                    ));
                }
                checked_relation_row(
                    &vector.old,
                    &positions,
                    old.modulus,
                    "cohomology relation old terms are not canonical",
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = self.checked_old_projection_rows(rows, old.modulus as u64)?;
        let class_position = positions.get(&class).copied().ok_or_else(|| {
            Error::InvalidInput("cohomology relation names an unknown old class".into())
        })?;
        let mut target = SparseVector::default();
        target.insert(class_position, 1);
        reduce_by_basis(&mut target, &rows, old.modulus as u64);
        Ok(target.is_zero())
    }

    fn validate_old_projection(&self, old: &CohomologySpace) -> Result<()> {
        self.validate_old_header(old)?;
        self.validate_old_ranks()
    }

    fn validate_old_header(&self, old: &CohomologySpace) -> Result<()> {
        if self.old_space != old.id
            || self.dimension != old.dimension
            || self.scale.to_bits() != old.scale.to_bits()
            || self.modulus != old.modulus
            || self.old_rank != old.rank()
        {
            return Err(Error::InvalidInput(
                "cohomology relation does not match its old space".into(),
            ));
        }
        Ok(())
    }

    fn validate_old_ranks(&self) -> Result<()> {
        if self.relation_rank != self.basis.len()
            || self.old_image_rank > self.old_rank
            || self.old_kernel_rank > self.old_rank
            || self.old_image_rank.checked_add(self.old_kernel_rank) != Some(self.old_rank)
            || self.new_kernel_rank > self.new_rank
        {
            return Err(Error::InvalidInput(
                "cohomology relation old projection ranks are inconsistent".into(),
            ));
        }
        let minimum = self
            .old_kernel_rank
            .checked_add(self.new_kernel_rank)
            .ok_or_else(|| Error::InvalidInput("cohomology relation rank overflows".into()))?;
        if self.relation_rank < minimum {
            return Err(Error::InvalidInput(
                "cohomology relation old projection ranks are inconsistent".into(),
            ));
        }
        let maximum = self
            .old_rank
            .checked_add(self.new_rank)
            .ok_or_else(|| Error::InvalidInput("cohomology relation rank overflows".into()))?;
        if self.relation_rank > maximum {
            return Err(Error::InvalidInput(
                "cohomology relation rank is out of range".into(),
            ));
        }
        Ok(())
    }

    fn checked_old_projection_rows(
        &self,
        rows: Vec<SparseVector>,
        modulus: u64,
    ) -> Result<Vec<SparseVector>> {
        let rows = rref(rows, modulus);
        let expected = self
            .relation_rank
            .checked_sub(self.new_kernel_rank)
            .ok_or_else(|| {
                Error::InvalidInput("cohomology relation rank is inconsistent".into())
            })?;
        if rows.len() != expected {
            return Err(Error::InvalidInput(
                "cohomology relation old projection rank is inconsistent".into(),
            ));
        }
        Ok(rows)
    }
}
