use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, KineticEdgeKey, Result, SparseDistanceMatrix};

use super::model::{
    CoverageEvaluation, CoverageFence, CoverageLimits, CoverageTriangleTerm, PlanarCoverageModel,
    cycle_pairs, validate_field,
};

/// Check the planar controlled-boundary criterion on an induced active graph.
///
/// `graph` contains every possible communication edge. `active_vertices`
/// selects the nodes present in this state. The fence must be active, and each
/// consecutive fence pair must be a graph edge.
pub fn evaluate_planar_coverage(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    fence: &CoverageFence,
    modulus: u32,
    model: PlanarCoverageModel,
    limits: CoverageLimits,
) -> Result<CoverageEvaluation> {
    validate_coverage_input(graph, active_vertices, fence, modulus, model, limits)?;
    let edges = active_edges(
        graph,
        active_vertices,
        model.broadcast_radius(),
        limits.max_edges,
    )?;
    check_fence_edges(fence, &edges)?;
    let triangles = flag_triangles(active_vertices, &edges, limits.max_triangles)?;
    check_matrix_size(edges.len(), triangles.len(), limits.max_matrix_entries)?;
    let target = fence_chain(fence, &edges, modulus)?;
    let solution = solve_boundary(&edges, &triangles, &target, modulus);
    let criterion_holds = solution.is_some();
    let witness = coverage_witness(solution.unwrap_or_default(), &triangles);
    Ok(CoverageEvaluation {
        criterion_holds,
        witness,
        active_vertices: active_vertices.len(),
        active_edges: edges.len(),
        active_triangles: triangles.len(),
    })
}

fn validate_coverage_input(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    fence: &CoverageFence,
    modulus: u32,
    model: PlanarCoverageModel,
    limits: CoverageLimits,
) -> Result<()> {
    validate_field(modulus)?;
    PlanarCoverageModel::new(model.broadcast_radius(), model.sensing_radius())?;
    if graph.len() > limits.max_vertices || graph.is_empty() {
        return Err(Error::InvalidInput(
            "coverage graph exceeds its vertex limit or is empty".into(),
        ));
    }
    if active_vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || active_vertices.iter().any(|vertex| *vertex >= graph.len())
    {
        return Err(Error::InvalidInput(
            "coverage active vertices are not canonical".into(),
        ));
    }
    if fence
        .vertices()
        .iter()
        .any(|vertex| active_vertices.binary_search(vertex).is_err())
    {
        return Err(Error::InvalidInput(
            "coverage fence contains an inactive vertex".into(),
        ));
    }
    Ok(())
}

fn active_edges(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    broadcast_radius: f64,
    maximum: usize,
) -> Result<Vec<KineticEdgeKey>> {
    let active: BTreeSet<_> = active_vertices.iter().copied().collect();
    let edges = graph
        .edges()
        .filter(|(u, v, distance)| {
            active.contains(u) && active.contains(v) && *distance <= broadcast_radius
        })
        .map(|(u, v, _)| KineticEdgeKey::new(u, v))
        .collect::<Vec<_>>();
    if edges.len() > maximum {
        return Err(Error::InvalidInput(
            "coverage active edges exceed their limit".into(),
        ));
    }
    Ok(edges)
}

fn check_fence_edges(fence: &CoverageFence, edges: &[KineticEdgeKey]) -> Result<()> {
    for edge in fence.edges() {
        if edges.binary_search(&edge).is_err() {
            return Err(Error::InvalidInput(
                "coverage graph omits a consecutive fence edge".into(),
            ));
        }
    }
    Ok(())
}

fn check_matrix_size(edges: usize, triangles: usize, maximum: usize) -> Result<()> {
    let entries = edges
        .checked_mul(triangles.saturating_add(1))
        .ok_or_else(|| Error::InvalidInput("coverage linear-system size overflows".into()))?;
    if entries > maximum {
        return Err(Error::InvalidInput(
            "coverage linear system exceeds its entry limit".into(),
        ));
    }
    Ok(())
}

fn coverage_witness(solution: Vec<u32>, triangles: &[[usize; 3]]) -> Vec<CoverageTriangleTerm> {
    solution
        .into_iter()
        .enumerate()
        .filter(|(_, coefficient)| *coefficient != 0)
        .map(|(index, coefficient)| {
            let [a, b, c] = triangles[index];
            CoverageTriangleTerm {
                a,
                b,
                c,
                coefficient,
            }
        })
        .collect()
}

fn flag_triangles(
    vertices: &[usize],
    edges: &[KineticEdgeKey],
    maximum: usize,
) -> Result<Vec<[usize; 3]>> {
    let edge_set: BTreeSet<_> = edges.iter().copied().collect();
    let mut triangles = Vec::new();
    for (first_position, &a) in vertices.iter().enumerate() {
        for (second_position, &b) in vertices.iter().enumerate().skip(first_position + 1) {
            if !edge_set.contains(&KineticEdgeKey::new(a, b)) {
                continue;
            }
            for &c in vertices.iter().skip(second_position + 1) {
                if edge_set.contains(&KineticEdgeKey::new(a, c))
                    && edge_set.contains(&KineticEdgeKey::new(b, c))
                {
                    triangles.push([a, b, c]);
                    if triangles.len() > maximum {
                        return Err(Error::InvalidInput(
                            "coverage active triangles exceed their limit".into(),
                        ));
                    }
                }
            }
        }
    }
    Ok(triangles)
}

fn fence_chain(fence: &CoverageFence, edges: &[KineticEdgeKey], modulus: u32) -> Result<Vec<u32>> {
    let positions: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect();
    let mut chain = vec![0u32; edges.len()];
    for (u, v) in cycle_pairs(fence.vertices()) {
        let position = positions[&KineticEdgeKey::new(u, v)];
        let coefficient = if u < v { 1 } else { modulus - 1 };
        chain[position] = add(chain[position], coefficient, modulus);
    }
    if chain.iter().all(|value| *value == 0) {
        return Err(Error::InvalidInput(
            "coverage fence cycle is zero in the declared field".into(),
        ));
    }
    Ok(chain)
}

fn solve_boundary(
    edges: &[KineticEdgeKey],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Option<Vec<u32>> {
    let mut rows = boundary_system(edges, triangles, target, modulus);
    let pivots = reduce_boundary_system(&mut rows, triangles.len(), modulus);
    if inconsistent_system(&rows, triangles.len()) {
        return None;
    }
    Some(boundary_solution(&rows, &pivots, triangles.len()))
}

fn boundary_system(
    edges: &[KineticEdgeKey],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Vec<Vec<u32>> {
    let positions: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect();
    let mut rows = vec![vec![0u32; triangles.len() + 1]; edges.len()];
    for (column, &[a, b, c]) in triangles.iter().enumerate() {
        rows[positions[&KineticEdgeKey::new(b, c)]][column] = 1;
        rows[positions[&KineticEdgeKey::new(a, c)]][column] = modulus - 1;
        rows[positions[&KineticEdgeKey::new(a, b)]][column] = 1;
    }
    for (row, value) in rows.iter_mut().zip(target) {
        row[triangles.len()] = *value;
    }
    rows
}

fn reduce_boundary_system(rows: &mut [Vec<u32>], columns: usize, modulus: u32) -> Vec<usize> {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..columns {
        let Some(found) = (pivot_row..rows.len()).find(|row| rows[*row][column] != 0) else {
            continue;
        };
        reduce_pivot_column(rows, pivot_row, found, column, modulus);
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    pivots
}

fn reduce_pivot_column(
    rows: &mut [Vec<u32>],
    pivot_row: usize,
    found: usize,
    column: usize,
    modulus: u32,
) {
    rows.swap(pivot_row, found);
    let inverse = inverse(rows[pivot_row][column], modulus);
    for value in &mut rows[pivot_row][column..] {
        *value = multiply(*value, inverse, modulus);
    }
    let pivot = rows[pivot_row][column..].to_vec();
    for (row_index, row) in rows.iter_mut().enumerate() {
        if row_index == pivot_row || row[column] == 0 {
            continue;
        }
        let factor = row[column];
        for (value, pivot_value) in row[column..].iter_mut().zip(&pivot) {
            *value = subtract(*value, multiply(factor, *pivot_value, modulus), modulus);
        }
    }
}

fn inconsistent_system(rows: &[Vec<u32>], columns: usize) -> bool {
    rows.iter()
        .any(|row| row[..columns].iter().all(|value| *value == 0) && row[columns] != 0)
}

fn boundary_solution(rows: &[Vec<u32>], pivots: &[usize], columns: usize) -> Vec<u32> {
    let mut solution = vec![0u32; columns];
    for (row, &column) in pivots.iter().enumerate() {
        solution[column] = rows[row][columns];
    }
    solution
}

fn add(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(modulus)) as u32
}

fn subtract(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(modulus) - u64::from(right)) % u64::from(modulus)) as u32
}

fn multiply(left: u32, right: u32, modulus: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(modulus)) as u32
}

fn inverse(value: u32, modulus: u32) -> u32 {
    let mut result = 1u64;
    let mut base = u64::from(value);
    let mut exponent = u64::from(modulus - 2);
    let modulus = u64::from(modulus);
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}
