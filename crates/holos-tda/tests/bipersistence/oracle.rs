use super::*;

#[derive(Clone)]
pub(crate) struct DirectSlice {
    active: Vec<bool>,
    edges: Vec<(usize, usize)>,
}

pub(crate) fn direct_slice(
    vertex_count: usize,
    edges: &[(usize, usize, f64)],
    scale: f64,
    minimum_degree: usize,
) -> DirectSlice {
    let mut degrees = vec![0usize; vertex_count];
    for &(u, v, value) in edges {
        if value <= scale {
            degrees[u] += 1;
            degrees[v] += 1;
        }
    }
    let active = degrees
        .into_iter()
        .map(|degree| degree >= minimum_degree)
        .collect::<Vec<_>>();
    let edges = edges
        .iter()
        .filter(|&&(u, v, value)| value <= scale && active[u] && active[v])
        .map(|&(u, v, _)| (u, v))
        .collect();
    DirectSlice { active, edges }
}

pub(crate) fn direct_h1_rank(vertex_count: usize, complex: &DirectSlice) -> usize {
    let vertices = complex.active.iter().filter(|active| **active).count();
    let components = component_count(vertex_count, complex);
    let cycle_rank = complex.edges.len() + components - vertices;
    cycle_rank - gf2_rank(triangle_boundaries(vertex_count, complex))
}

pub(crate) fn direct_inclusion_rank(
    vertex_count: usize,
    lower: &DirectSlice,
    upper: &DirectSlice,
) -> usize {
    let cycles = cycle_basis(vertex_count, lower);
    let boundaries = triangle_boundaries(vertex_count, upper);
    gf2_rank(
        cycles
            .into_iter()
            .chain(boundaries.iter().copied())
            .collect(),
    ) - gf2_rank(boundaries)
}

#[derive(Clone)]
pub(crate) struct DirectHomologyNode {
    basis: Vec<u64>,
    boundaries: Vec<u64>,
}

pub(crate) fn direct_rectangle_rank(
    vertex_count: usize,
    edges: &[(usize, usize, f64)],
    scales: &[f64],
    minimum_degrees: &[usize],
) -> usize {
    let grades = (0..scales.len())
        .flat_map(|scale| {
            (0..minimum_degrees.len()).map(move |density| Bigrade::new(scale, density))
        })
        .collect::<Vec<_>>();
    direct_region_rank(vertex_count, edges, scales, minimum_degrees, &grades)
}

pub(crate) fn direct_region_rank(
    vertex_count: usize,
    edges: &[(usize, usize, f64)],
    scales: &[f64],
    minimum_degrees: &[usize],
    grades: &[Bigrade],
) -> usize {
    let node_count = grades.len();
    let mut nodes = Vec::with_capacity(node_count);
    for &grade in grades {
        let slice = direct_slice(
            vertex_count,
            edges,
            scales[grade.scale()],
            minimum_degrees[grade.density()],
        );
        let boundaries = triangle_boundaries(vertex_count, &slice);
        let basis = direct_homology_basis(vertex_count, &slice, &boundaries);
        nodes.push(DirectHomologyNode { basis, boundaries });
    }

    let mut offsets = Vec::with_capacity(node_count);
    let mut ambient_rank = 0usize;
    for node in &nodes {
        offsets.push(ambient_rank);
        ambient_rank += node.basis.len();
    }
    assert!(ambient_rank < 63, "oracle direct sum is too large for u64");
    if ambient_rank == 0 {
        return 0;
    }

    let mut relations = Vec::new();
    let mut equations = Vec::new();
    for (source, &lower) in grades.iter().enumerate() {
        for (target, &upper) in grades.iter().enumerate().skip(source + 1) {
            if lower.precedes(upper) {
                add_direct_cover(
                    source,
                    target,
                    &nodes,
                    &offsets,
                    &mut relations,
                    &mut equations,
                );
            }
        }
    }

    let limit_vectors = (0..(1u64 << ambient_rank))
        .filter(|vector| equations.iter().all(|row| parity(row & vector) == 0))
        .map(|vector| vector & ((1u64 << nodes[0].basis.len()) - 1))
        .collect::<Vec<_>>();
    let relation_rank = gf2_rank(relations.clone());
    let union_rank = gf2_rank(relations.into_iter().chain(limit_vectors).collect());
    union_rank - relation_rank
}

pub(crate) fn direct_homology_basis(
    vertex_count: usize,
    slice: &DirectSlice,
    boundaries: &[u64],
) -> Vec<u64> {
    let mut basis = Vec::new();
    let mut rank = gf2_rank(boundaries.to_vec());
    for cycle in cycle_basis(vertex_count, slice) {
        let mut extension = boundaries.to_vec();
        extension.extend(basis.iter().copied());
        extension.push(cycle);
        let extended_rank = gf2_rank(extension);
        if extended_rank > rank {
            basis.push(cycle);
            rank = extended_rank;
        }
    }
    basis
}

pub(crate) fn add_direct_cover(
    source_index: usize,
    target_index: usize,
    nodes: &[DirectHomologyNode],
    offsets: &[usize],
    relations: &mut Vec<u64>,
    equations: &mut Vec<u64>,
) {
    let source = &nodes[source_index];
    let target = &nodes[target_index];
    let map = source
        .basis
        .iter()
        .map(|&cycle| quotient_coordinates(cycle, &target.basis, &target.boundaries))
        .collect::<Vec<_>>();
    for (source_basis, image) in map.iter().copied().enumerate() {
        let source_bit = 1u64 << (offsets[source_index] + source_basis);
        let mut image_vector = 0u64;
        if image != 0 {
            for target_basis in 0..target.basis.len() {
                if image & (1u64 << target_basis) != 0 {
                    image_vector |= 1u64 << (offsets[target_index] + target_basis);
                }
            }
        }
        relations.push(source_bit | image_vector);
    }
    for target_basis in 0..target.basis.len() {
        let target_bit = 1u64 << (offsets[target_index] + target_basis);
        let mut equation = target_bit;
        for (source_basis, image) in map.iter().copied().enumerate() {
            if image & (1u64 << target_basis) != 0 {
                equation |= 1u64 << (offsets[source_index] + source_basis);
            }
        }
        equations.push(equation);
    }
}

pub(crate) fn quotient_coordinates(vector: u64, basis: &[u64], boundaries: &[u64]) -> u64 {
    for coefficients in 0u64..(1u64 << basis.len()) {
        let mut remainder = vector;
        for (position, &element) in basis.iter().enumerate() {
            if coefficients & (1u64 << position) != 0 {
                remainder ^= element;
            }
        }
        let mut span = boundaries.to_vec();
        span.push(remainder);
        if gf2_rank(span) == gf2_rank(boundaries.to_vec()) {
            return coefficients;
        }
    }
    panic!("cycle does not define a class in the target homology quotient");
}

pub(crate) fn parity(value: u64) -> u32 {
    value.count_ones() & 1
}

pub(crate) fn component_count(vertex_count: usize, complex: &DirectSlice) -> usize {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for &(u, v) in &complex.edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    let mut seen = vec![false; vertex_count];
    let mut components = 0usize;
    for start in 0..vertex_count {
        if !complex.active[start] || seen[start] {
            continue;
        }
        components += 1;
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(vertex) = stack.pop() {
            for &neighbor in &adjacency[vertex] {
                if !seen[neighbor] {
                    seen[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
    }
    components
}

pub(crate) fn cycle_basis(vertex_count: usize, complex: &DirectSlice) -> Vec<u64> {
    let edge_positions = complete_edge_positions(vertex_count);
    let mut cycles = Vec::new();
    for subset in 0u64..1 << complex.edges.len() {
        let mut parity = vec![false; vertex_count];
        let mut vector = 0u64;
        for (position, &(u, v)) in complex.edges.iter().enumerate() {
            if subset & (1 << position) != 0 {
                parity[u] ^= true;
                parity[v] ^= true;
                vector |= 1 << edge_positions[&(u, v)];
            }
        }
        if parity.into_iter().all(|value| !value) {
            cycles.push(vector);
        }
    }
    gf2_basis(cycles)
}

pub(crate) fn triangle_boundaries(vertex_count: usize, complex: &DirectSlice) -> Vec<u64> {
    let positions = complete_edge_positions(vertex_count);
    let edges = complex
        .edges
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut boundaries = Vec::new();
    for u in 0..vertex_count {
        for v in u + 1..vertex_count {
            for w in v + 1..vertex_count {
                if edges.contains(&(u, v)) && edges.contains(&(u, w)) && edges.contains(&(v, w)) {
                    boundaries.push(
                        (1 << positions[&(u, v)])
                            | (1 << positions[&(u, w)])
                            | (1 << positions[&(v, w)]),
                    );
                }
            }
        }
    }
    boundaries
}

pub(crate) fn complete_edge_positions(
    vertex_count: usize,
) -> std::collections::BTreeMap<(usize, usize), usize> {
    let mut positions = std::collections::BTreeMap::new();
    for v in 1..vertex_count {
        for u in 0..v {
            positions.insert((u, v), positions.len());
        }
    }
    positions
}

pub(crate) fn gf2_rank(rows: Vec<u64>) -> usize {
    gf2_basis(rows).len()
}

pub(crate) fn gf2_basis(rows: Vec<u64>) -> Vec<u64> {
    let mut basis = [0u64; 64];
    for mut row in rows {
        while row != 0 {
            let pivot = row.trailing_zeros() as usize;
            if basis[pivot] == 0 {
                basis[pivot] = row;
                break;
            }
            row ^= basis[pivot];
        }
    }
    basis.into_iter().filter(|row| *row != 0).collect()
}

pub(crate) fn graph_from_binary_mask(vertex_count: usize, mask: u64) -> Vec<(usize, usize, f64)> {
    complete_edges(vertex_count)
        .into_iter()
        .enumerate()
        .filter(|(position, _)| mask & (1 << position) != 0)
        .map(|(_, (u, v))| (u, v, 1.0))
        .collect()
}

pub(crate) fn graph_from_ternary_code(
    vertex_count: usize,
    mut code: u64,
) -> Vec<(usize, usize, f64)> {
    let mut edges = Vec::new();
    for (u, v) in complete_edges(vertex_count) {
        let digit = code % 3;
        code /= 3;
        if digit != 0 {
            edges.push((u, v, digit as f64));
        }
    }
    edges
}

pub(crate) fn complete_edges(vertex_count: usize) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for v in 1..vertex_count {
        for u in 0..v {
            edges.push((u, v));
        }
    }
    edges
}
