//! Comparable maps and generalized rectangle-rank queries.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result};

use super::linear::{LinearMap, SparseVector, checked_term_sum, negate, nullspace, rank};
use super::{
    Bigrade, BipersistenceMap, BipersistenceModule, BipersistenceRectangle, BipersistenceRegion,
};

impl BipersistenceModule {
    /// Compute the exact map between comparable grades.
    ///
    /// The result composes cover maps. Checked commutative squares make the
    /// result independent of the chosen monotone path.
    pub fn map(&self, lower: Bigrade, upper: Bigrade) -> Result<BipersistenceMap> {
        self.validate_comparable(lower, upper)?;
        let upper_node = self.node(upper)?;
        let mut composed = LinearMap::identity(upper_node.rank);
        let mut current = upper;
        while current.scale() > lower.scale() {
            let next = Bigrade::new(current.scale() - 1, current.density());
            let cover = self.cover_linear(next, current)?;
            composed = LinearMap::compose(&cover, &composed, self.modulus)?;
            current = next;
        }
        while current.density() > lower.density() {
            let next = Bigrade::new(current.scale(), current.density() - 1);
            let cover = self.cover_linear(next, current)?;
            composed = LinearMap::compose(&cover, &composed, self.modulus)?;
            current = next;
        }
        Ok(self.public_map(lower, upper, &composed))
    }

    /// Rank of the exact map between comparable grades.
    pub fn map_rank(&self, lower: Bigrade, upper: Bigrade) -> Result<usize> {
        Ok(self.map(lower, upper)?.rank)
    }

    /// Compute the generalized rank of one closed rectangle.
    ///
    /// The value is the rank of the canonical map from the diagram limit to
    /// its colimit. The same value is obtained from the dual homology module.
    pub fn rectangle_rank(&self, rectangle: BipersistenceRectangle) -> Result<usize> {
        self.validate_comparable(rectangle.lower, rectangle.upper)?;
        let region = BipersistenceRegion::new(rectangle_grades(rectangle))?;
        self.region_rank(&region)
    }

    /// Compute the generalized rank of one connected finite region.
    ///
    /// The value is the rank of the canonical map from the diagram limit to
    /// its colimit. Every comparable pair in the induced subposet contributes
    /// its exact module map. A region with a minimum and maximum has the rank
    /// of the map between those grades.
    pub fn region_rank(&self, region: &BipersistenceRegion) -> Result<usize> {
        self.validate_region(region)?;
        let grades = region.grades();
        let (offsets, ambient_rank) = self.region_offsets(grades)?;
        if ambient_rank > self.limits.max_linear_variables {
            return Err(Error::InvalidInput(format!(
                "region direct-sum rank exceeds the limit {}",
                self.limits.max_linear_variables
            )));
        }
        if ambient_rank == 0 {
            return Ok(0);
        }
        let (relations, equations) = self.region_relations(grades, &offsets)?;
        let limit = nullspace(equations, ambient_rank, self.modulus);
        let first = grades[0];
        let first_offset = offsets[&first];
        let first_rank = self.node(first)?.rank;
        let images = limit_images(limit, first_offset, first_rank, self.modulus);
        let relation_rank = rank(relations.clone(), ambient_rank, self.modulus);
        let union_rank = rank(
            relations.into_iter().chain(images).collect(),
            ambient_rank,
            self.modulus,
        );
        Ok(union_rank - relation_rank)
    }

    fn region_offsets(&self, grades: &[Bigrade]) -> Result<(BTreeMap<Bigrade, usize>, usize)> {
        let mut offsets = BTreeMap::new();
        let mut ambient_rank = 0usize;
        for grade in grades {
            offsets.insert(*grade, ambient_rank);
            ambient_rank = ambient_rank
                .checked_add(self.node(*grade)?.rank)
                .ok_or_else(|| Error::InvalidInput("region direct-sum rank overflows".into()))?;
        }
        Ok((offsets, ambient_rank))
    }

    fn region_relations(
        &self,
        grades: &[Bigrade],
        offsets: &BTreeMap<Bigrade, usize>,
    ) -> Result<(Vec<SparseVector>, Vec<SparseVector>)> {
        let mut relations = Vec::new();
        let mut equations = Vec::new();
        let mut coefficient_count = 0usize;
        for (lower, upper) in comparable_pairs(grades) {
            let map = self.map(lower, upper)?;
            let linear = self.linear_from_public(&map)?;
            let source_offset = offsets[&upper];
            let target_offset = offsets[&lower];
            append_map_relations(
                &mut relations,
                &linear,
                source_offset,
                target_offset,
                self.modulus,
                &mut coefficient_count,
                self.limits,
            )?;
            append_map_equations(
                &mut equations,
                &linear,
                source_offset,
                target_offset,
                self.modulus,
                &mut coefficient_count,
                self.limits,
            )?;
        }
        Ok((relations, equations))
    }

    fn validate_region(&self, region: &BipersistenceRegion) -> Result<()> {
        for &grade in region.grades() {
            self.node(grade)?;
        }
        let mut reached = BTreeSet::from([region.grades()[0]]);
        loop {
            let before = reached.len();
            for &grade in region.grades() {
                if region.grades().iter().copied().any(|other| {
                    reached.contains(&other) && (grade.precedes(other) || other.precedes(grade))
                }) {
                    reached.insert(grade);
                }
            }
            if reached.len() == region.grades().len() {
                return Ok(());
            }
            if reached.len() == before {
                return Err(Error::InvalidInput(
                    "a bipersistence region must have a connected comparability graph".into(),
                ));
            }
        }
    }
}

fn comparable_pairs(grades: &[Bigrade]) -> Vec<(Bigrade, Bigrade)> {
    let mut pairs = Vec::new();
    for (position, &lower) in grades.iter().enumerate() {
        for &upper in &grades[position + 1..] {
            if lower.precedes(upper) {
                pairs.push((lower, upper));
            }
        }
    }
    pairs
}

fn rectangle_grades(rectangle: BipersistenceRectangle) -> Vec<Bigrade> {
    let mut grades = Vec::new();
    for scale in rectangle.lower.scale()..=rectangle.upper.scale() {
        for density in rectangle.lower.density()..=rectangle.upper.density() {
            grades.push(Bigrade::new(scale, density));
        }
    }
    grades
}

fn append_map_relations(
    relations: &mut Vec<SparseVector>,
    linear: &LinearMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
    coefficient_count: &mut usize,
    limits: super::BipersistenceLimits,
) -> Result<()> {
    for (source, column) in linear.columns.iter().enumerate() {
        let mut relation = SparseVector::default();
        relation.insert(source_offset + source, 1, modulus);
        for (&target, &coefficient) in &column.0 {
            relation.insert(
                target_offset + target,
                negate(coefficient, modulus),
                modulus,
            );
        }
        *coefficient_count = checked_term_sum(*coefficient_count, relation.len(), limits)?;
        relations.push(relation);
    }
    Ok(())
}

fn append_map_equations(
    equations: &mut Vec<SparseVector>,
    linear: &LinearMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
    coefficient_count: &mut usize,
    limits: super::BipersistenceLimits,
) -> Result<()> {
    for target in 0..linear.target_rank {
        let mut equation = SparseVector::default();
        equation.insert(target_offset + target, 1, modulus);
        for (source, column) in linear.columns.iter().enumerate() {
            if let Some(&coefficient) = column.0.get(&target) {
                equation.insert(
                    source_offset + source,
                    negate(coefficient, modulus),
                    modulus,
                );
            }
        }
        *coefficient_count = checked_term_sum(*coefficient_count, equation.len(), limits)?;
        equations.push(equation);
    }
    Ok(())
}

fn limit_images(
    limit: Vec<SparseVector>,
    first_offset: usize,
    first_rank: usize,
    modulus: u32,
) -> Vec<SparseVector> {
    limit
        .into_iter()
        .map(|vector| {
            let mut image = SparseVector::default();
            for (&position, &coefficient) in vector.0.range(first_offset..first_offset + first_rank)
            {
                image.insert(position, coefficient, modulus);
            }
            image
        })
        .collect::<Vec<_>>()
}
