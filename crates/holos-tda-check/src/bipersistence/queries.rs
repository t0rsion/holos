use std::collections::{BTreeMap, BTreeSet};

use crate::ProofError;

use super::BipersistenceProofLimits;
use super::claims::Grade;
use super::linear::{LinearMap, Vector, negate, nullspace, rank};
use super::module::CheckedModule;

impl CheckedModule {
    pub(crate) fn rectangle_rank(
        &self,
        lower: Grade,
        upper: Grade,
        limits: BipersistenceProofLimits,
    ) -> Result<usize, ProofError> {
        self.validate_comparable(lower, upper)?;
        let grades = rectangle_grades(lower, upper);
        self.region_rank(&grades, limits)
    }

    pub(crate) fn region_rank(
        &self,
        grades: &[Grade],
        limits: BipersistenceProofLimits,
    ) -> Result<usize, ProofError> {
        self.validate_region(grades)?;
        let (offsets, ambient) = self.region_offsets(grades, limits)?;
        if ambient > limits.max_linear_variables {
            return Err(ProofError::new("region direct-sum rank exceeds its limit"));
        }
        if ambient == 0 {
            return Ok(0);
        }
        let (relations, equations) = self.region_system(grades, &offsets, limits)?;
        let limit = nullspace(equations, ambient, self.modulus);
        let first = grades[0];
        let offset = offsets[&first];
        let first_rank = self.node(first)?.space.rank();
        let images = project_limit(limit, offset, first_rank, self.modulus);
        let relation_rank = rank(relations.clone(), ambient, self.modulus);
        let union_rank = rank(
            relations.into_iter().chain(images).collect(),
            ambient,
            self.modulus,
        );
        Ok(union_rank - relation_rank)
    }

    fn region_offsets(
        &self,
        grades: &[Grade],
        limits: BipersistenceProofLimits,
    ) -> Result<(BTreeMap<Grade, usize>, usize), ProofError> {
        let mut offsets = BTreeMap::new();
        let mut ambient = 0usize;
        for grade in grades {
            offsets.insert(*grade, ambient);
            ambient = ambient
                .checked_add(self.node(*grade)?.space.rank())
                .ok_or_else(|| ProofError::new("region direct-sum rank overflows"))?;
        }
        if ambient > limits.max_linear_variables {
            return Err(ProofError::new("region direct-sum rank exceeds its limit"));
        }
        Ok((offsets, ambient))
    }

    fn region_system(
        &self,
        grades: &[Grade],
        offsets: &BTreeMap<Grade, usize>,
        limits: BipersistenceProofLimits,
    ) -> Result<(Vec<Vector>, Vec<Vector>), ProofError> {
        let mut relations = Vec::new();
        let mut equations = Vec::new();
        let mut terms = 0usize;
        for (lower, upper) in comparable_pairs(grades) {
            let linear = self.map(lower, upper)?;
            let source_offset = offsets[&upper];
            let target_offset = offsets[&lower];
            terms = append_relations(
                &mut relations,
                &linear,
                source_offset,
                target_offset,
                self.modulus,
                terms,
                limits.max_linear_terms,
            )?;
            terms = append_equations(
                &mut equations,
                &linear,
                source_offset,
                target_offset,
                self.modulus,
                terms,
                limits.max_linear_terms,
            )?;
        }
        Ok((relations, equations))
    }

    fn validate_region(&self, grades: &[Grade]) -> Result<(), ProofError> {
        if grades.is_empty() || grades.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ProofError::new(
                "bipersistence region grades are not canonical",
            ));
        }
        for &grade in grades {
            self.validate_grade(grade)?;
        }
        let mut reached = BTreeSet::from([grades[0]]);
        loop {
            let before = reached.len();
            for &grade in grades {
                if grades.iter().copied().any(|other| {
                    reached.contains(&other) && (grade.precedes(other) || other.precedes(grade))
                }) {
                    reached.insert(grade);
                }
            }
            if reached.len() == grades.len() {
                return Ok(());
            }
            if reached.len() == before {
                return Err(ProofError::new(
                    "bipersistence region comparability graph is disconnected",
                ));
            }
        }
    }
}

fn comparable_pairs(grades: &[Grade]) -> Vec<(Grade, Grade)> {
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

fn rectangle_grades(lower: Grade, upper: Grade) -> Vec<Grade> {
    let mut grades = Vec::new();
    for scale in lower.scale..=upper.scale {
        for density in lower.density..=upper.density {
            grades.push(Grade { scale, density });
        }
    }
    grades
}

fn append_relations(
    relations: &mut Vec<Vector>,
    linear: &LinearMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
    current_terms: usize,
    maximum_terms: usize,
) -> Result<usize, ProofError> {
    let mut terms = current_terms;
    for (source, column) in linear.columns.iter().enumerate() {
        let mut relation = Vector::default();
        relation.insert(source_offset + source, 1, modulus);
        for (&target, &coefficient) in &column.0 {
            relation.insert(
                target_offset + target,
                negate(coefficient, modulus),
                modulus,
            );
        }
        terms = bounded_terms(terms, relation.0.len(), maximum_terms)?;
        relations.push(relation);
    }
    Ok(terms)
}

fn append_equations(
    equations: &mut Vec<Vector>,
    linear: &LinearMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
    current_terms: usize,
    maximum_terms: usize,
) -> Result<usize, ProofError> {
    let mut terms = current_terms;
    for target in 0..linear.target_rank {
        let mut equation = Vector::default();
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
        terms = bounded_terms(terms, equation.0.len(), maximum_terms)?;
        equations.push(equation);
    }
    Ok(terms)
}

fn project_limit(
    limit: Vec<Vector>,
    offset: usize,
    first_rank: usize,
    modulus: u32,
) -> Vec<Vector> {
    limit
        .into_iter()
        .map(|vector| project_vector(&vector, offset, first_rank, modulus))
        .collect()
}

fn project_vector(vector: &Vector, offset: usize, rank: usize, modulus: u32) -> Vector {
    let mut image = Vector::default();
    for (&position, &coefficient) in vector.0.range(offset..offset + rank) {
        image.insert(position, coefficient, modulus);
    }
    image
}

fn bounded_terms(current: usize, added: usize, maximum: usize) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new("region coefficient count overflows"))?;
    if next > maximum {
        Err(ProofError::new(
            "region coefficient count exceeds its limit",
        ))
    } else {
        Ok(next)
    }
}
