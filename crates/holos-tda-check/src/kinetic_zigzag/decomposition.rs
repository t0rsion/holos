use std::collections::BTreeSet;

use crate::cohomology::MapTerm;
use crate::{ProofError, inverse_mod};

use super::model::{Interval, Map, Vector};

pub(crate) fn decompose(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
) -> Result<(Vec<usize>, Vec<Interval>), ProofError> {
    let ranks = compute_generalized_ranks(dimensions, maps, modulus)?;
    let intervals = interval_decomposition(&ranks, dimensions.len())?;
    Ok((ranks, intervals))
}

fn compute_generalized_ranks(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
) -> Result<Vec<usize>, ProofError> {
    let count = dimensions.len();
    let mut ranks = vec![0usize; count * count];
    let mut work = 0usize;
    for start in (0..count).rev() {
        for end in start..count {
            work = add_rank_work(work, dimensions, maps, start, end)?;
            ranks[start * count + end] = generalized_rank(dimensions, maps, modulus, start, end);
        }
    }
    Ok(ranks)
}

fn add_rank_work(
    work: usize,
    dimensions: &[usize],
    maps: &[Map],
    start: usize,
    end: usize,
) -> Result<usize, ProofError> {
    let ambient = dimensions[start..=end]
        .iter()
        .try_fold(0usize, |sum, dimension| sum.checked_add(*dimension));
    let arrows = (start..end).try_fold(0usize, |sum, position| {
        let source = maps[position].columns.len();
        let target = if maps[position].forward {
            dimensions[position + 1]
        } else {
            dimensions[position]
        };
        sum.checked_add(source)?.checked_add(target)
    });
    let next = ambient
        .and_then(|value| arrows.and_then(|arrow_work| value.checked_add(arrow_work)))
        .and_then(|value| work.checked_add(value))
        .ok_or_else(|| ProofError::new("kinetic zigzag rank work overflows"))?;
    if next > super::model::FORMAT_MAX_RANK_WORK {
        Err(ProofError::new(
            "kinetic zigzag rank work exceeds the format limit",
        ))
    } else {
        Ok(next)
    }
}

fn interval_decomposition(ranks: &[usize], count: usize) -> Result<Vec<Interval>, ProofError> {
    let mut intervals = Vec::new();
    for start in 0..count {
        for end in start..count {
            let multiplicity = interval_multiplicity(ranks, count, start, end);
            if multiplicity < 0 {
                return Err(ProofError::new(
                    "kinetic zigzag generalized ranks are not interval decomposable",
                ));
            }
            if multiplicity > 0 {
                intervals.push((start, end, multiplicity as usize));
            }
        }
    }
    Ok(intervals)
}

fn interval_multiplicity(ranks: &[usize], count: usize, start: usize, end: usize) -> i128 {
    let rank = |left: usize, right: usize| ranks[left * count + right] as i128;
    let mut multiplicity = rank(start, end);
    if start > 0 {
        multiplicity -= rank(start - 1, end);
    }
    if end + 1 < count {
        multiplicity -= rank(start, end + 1);
    }
    if start > 0 && end + 1 < count {
        multiplicity += rank(start - 1, end + 1);
    }
    multiplicity
}

fn generalized_rank(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
    start: usize,
    end: usize,
) -> usize {
    if start == end {
        return dimensions[start];
    }
    let modulus = modulus as u64;
    let mut offsets = vec![0usize; end - start + 1];
    for position in 1..offsets.len() {
        offsets[position] = offsets[position - 1] + dimensions[start + position - 1];
    }
    let ambient = offsets.last().copied().unwrap_or(0) + dimensions[end];
    let mut relations = Vec::new();
    let mut equations = Vec::new();
    for position in start..end {
        append_map_constraints(
            &mut relations,
            &mut equations,
            dimensions,
            maps,
            &offsets,
            start,
            position,
            modulus,
        );
    }
    let relation_rank = rref(relations.clone(), modulus).len();
    let limit = nullspace(equations, ambient, modulus);
    append_limit_images(&mut relations, limit, dimensions[start]);
    rref(relations, modulus).len() - relation_rank
}

#[allow(clippy::too_many_arguments)]
fn append_map_constraints(
    relations: &mut Vec<Vector>,
    equations: &mut Vec<Vector>,
    dimensions: &[usize],
    maps: &[Map],
    offsets: &[usize],
    start: usize,
    position: usize,
    modulus: u64,
) {
    let map = &maps[position];
    let left = offsets[position - start];
    let right = offsets[position + 1 - start];
    let (source, target, target_dimension) = if map.forward {
        (left, right, dimensions[position + 1])
    } else {
        (right, left, dimensions[position])
    };
    for (column, terms) in map.columns.iter().enumerate() {
        relations.push(map_relation(source, target, column, terms, modulus));
    }
    for target_position in 0..target_dimension {
        equations.push(map_equation(
            source,
            target,
            target_position,
            &map.columns,
            modulus,
        ));
    }
}

fn map_relation(
    source: usize,
    target: usize,
    column: usize,
    terms: &[MapTerm],
    modulus: u64,
) -> Vector {
    let mut relation = Vector::default();
    relation.insert(source + column, 1);
    for term in terms {
        relation.insert(
            target + term.target,
            (modulus - u64::from(term.coefficient)) as u32,
        );
    }
    relation
}

fn map_equation(
    source: usize,
    target: usize,
    target_position: usize,
    columns: &[Vec<MapTerm>],
    modulus: u64,
) -> Vector {
    let mut equation = Vector::default();
    equation.insert(target + target_position, 1);
    for (column, terms) in columns.iter().enumerate() {
        if let Ok(term) = terms.binary_search_by_key(&target_position, |item| item.target) {
            equation.insert(
                source + column,
                (modulus - u64::from(terms[term].coefficient)) as u32,
            );
        }
    }
    equation
}

fn append_limit_images(relations: &mut Vec<Vector>, limit: Vec<Vector>, dimension: usize) {
    for section in limit {
        let mut image = Vector::default();
        for (&position, &coefficient) in section.0.range(..dimension) {
            image.insert(position, coefficient);
        }
        if !image.is_zero() {
            relations.push(image);
        }
    }
}

fn rref(rows: Vec<Vector>, modulus: u64) -> Vec<Vector> {
    let mut basis: Vec<Vector> = Vec::new();
    for mut row in rows {
        reduce(&mut row, &basis, modulus);
        let Some((pivot, coefficient)) = row.leading() else {
            continue;
        };
        row.scale(inverse_mod(u64::from(coefficient), modulus), modulus);
        for existing in &mut basis {
            if let Some(&coefficient) = existing.0.get(&pivot) {
                existing.add_scaled(&row, modulus - u64::from(coefficient), modulus);
            }
        }
        basis.push(row);
        basis.sort_by_key(|row| row.leading().map(|(position, _)| position));
    }
    basis
}

fn reduce(row: &mut Vector, basis: &[Vector], modulus: u64) {
    for existing in basis {
        let pivot = existing.leading().unwrap().0;
        if let Some(&coefficient) = row.0.get(&pivot) {
            row.add_scaled(existing, modulus - u64::from(coefficient), modulus);
        }
    }
}

fn nullspace(equations: Vec<Vector>, variables: usize, modulus: u64) -> Vec<Vector> {
    let equations = rref(equations, modulus);
    let pivots = equations
        .iter()
        .map(|row| row.leading().unwrap().0)
        .collect::<BTreeSet<_>>();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|position| !pivots.contains(position)) {
        let mut vector = Vector::default();
        vector.insert(free, 1);
        for equation in &equations {
            if let Some(&coefficient) = equation.0.get(&free) {
                vector.insert(
                    equation.leading().unwrap().0,
                    (modulus - u64::from(coefficient)) as u32,
                );
            }
        }
        basis.push(vector);
    }
    basis
}
