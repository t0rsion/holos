use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{Error, Result, SparseDistanceMatrix};

use super::algebra::{
    checked_coordinate_vector, coordinates_in_basis, reduce_by_basis, relation_row, rref,
};
use super::digest::{active_graph_digest, write_hex};
use super::model::*;

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
        let rows = image_rows(&self.columns, &positions);
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
        let image = image_rows(&self.columns, &positions);
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

fn image_rows(
    columns: &[CohomologyMapColumn],
    positions: &BTreeMap<CohomologyClassId, usize>,
) -> Vec<SparseVector> {
    columns
        .iter()
        .map(|column| {
            let mut row = SparseVector::default();
            for term in &column.image {
                row.insert(positions[&term.class], term.coefficient);
            }
            row
        })
        .collect()
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
