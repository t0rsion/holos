use super::algebra::{Vector, nullspace, rank};
use super::{ZigzagDirection, ZigzagMap, ZigzagModule};
use crate::Result;

pub(super) fn generalized_rank(module: &ZigzagModule, start: usize, end: usize) -> Result<usize> {
    if start == end {
        return Ok(module.dimensions[start]);
    }
    let modulus = module.modulus as u64;
    let offsets = node_offsets(&module.dimensions, start, end);
    let ambient = offsets.last().copied().unwrap_or(0) + module.dimensions[end];
    let (mut relations, equations) = compatibility_system(module, start, end, &offsets);
    let relation_rank = rank(relations.clone(), modulus);
    let limit = nullspace(equations, ambient, modulus);
    relations.extend(first_node_images(limit, module.dimensions[start]));
    Ok(rank(relations, modulus) - relation_rank)
}

fn node_offsets(dimensions: &[usize], start: usize, end: usize) -> Vec<usize> {
    let mut offsets = vec![0usize; end - start + 1];
    for position in 1..offsets.len() {
        offsets[position] = offsets[position - 1] + dimensions[start + position - 1];
    }
    offsets
}

fn compatibility_system(
    module: &ZigzagModule,
    start: usize,
    end: usize,
    offsets: &[usize],
) -> (Vec<Vector>, Vec<Vector>) {
    let mut relations = Vec::new();
    let mut equations = Vec::new();
    for position in start..end {
        let map = &module.maps[position];
        let left_offset = offsets[position - start];
        let right_offset = offsets[position + 1 - start];
        let shape = oriented_offsets(module, position, left_offset, right_offset);
        relations.extend(map_relations(map, shape.0, shape.1, module.modulus));
        equations.extend(map_equations(
            map,
            shape.0,
            shape.1,
            shape.2,
            module.modulus,
        ));
    }
    (relations, equations)
}

fn oriented_offsets(
    module: &ZigzagModule,
    position: usize,
    left_offset: usize,
    right_offset: usize,
) -> (usize, usize, usize) {
    match module.maps[position].direction {
        ZigzagDirection::Forward => (left_offset, right_offset, module.dimensions[position + 1]),
        ZigzagDirection::Backward => (right_offset, left_offset, module.dimensions[position]),
    }
}

fn map_relations(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
) -> Vec<Vector> {
    map.columns
        .iter()
        .enumerate()
        .map(|(column, terms)| {
            let mut relation = Vector::default();
            relation.insert(source_offset + column, 1);
            for term in terms {
                relation.insert(
                    target_offset + term.target,
                    (u64::from(modulus) - u64::from(term.coefficient)) as u32,
                );
            }
            relation
        })
        .collect()
}

fn map_equations(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    target_dimension: usize,
    modulus: u32,
) -> Vec<Vector> {
    (0..target_dimension)
        .map(|target| map_equation(map, source_offset, target_offset, target, modulus))
        .collect()
}

fn map_equation(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    target: usize,
    modulus: u32,
) -> Vector {
    let mut equation = Vector::default();
    equation.insert(target_offset + target, 1);
    for (column, terms) in map.columns.iter().enumerate() {
        if let Ok(term_position) = terms.binary_search_by_key(&target, |term| term.target) {
            let coefficient = terms[term_position].coefficient;
            equation.insert(
                source_offset + column,
                (u64::from(modulus) - u64::from(coefficient)) as u32,
            );
        }
    }
    equation
}

fn first_node_images(limit: Vec<Vector>, first_dimension: usize) -> impl Iterator<Item = Vector> {
    limit.into_iter().filter_map(move |section| {
        let mut image = Vector::default();
        for (&position, &coefficient) in section.0.range(..first_dimension) {
            image.insert(position, coefficient);
        }
        (!image.is_zero()).then_some(image)
    })
}
