use std::collections::{BTreeMap, VecDeque};

use crate::{Error, Result};

use super::algebra::nullspace;
use super::model::SparseVector;

/// Build closed edge cochains in the unique gauge that vanishes on a
/// spanning forest of the active graph.
pub(crate) fn forest_cocycles(
    vertex_count: usize,
    edges: &[Vec<usize>],
    triangles: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<SparseVector>> {
    let edge_positions = edge_positions(vertex_count, edges)?;
    let tree_edges = spanning_forest(vertex_count, &edge_positions, edges.len());
    let mut chord_positions = vec![usize::MAX; edges.len()];
    let mut chords = Vec::new();
    for (position, &is_tree) in tree_edges.iter().enumerate() {
        if !is_tree {
            chord_positions[position] = chords.len();
            chords.push(position);
        }
    }

    let equations = triangles
        .iter()
        .map(|triangle| {
            validate_triangle(vertex_count, triangle)?;
            let mut equation = SparseVector::default();
            for removed in 0..triangle.len() {
                let mut edge = triangle.clone();
                edge.remove(removed);
                let edge_position = edge_positions
                    .get(&(edge[0], edge[1]))
                    .copied()
                    .ok_or_else(|| {
                        Error::InvalidInput("cohomology triangle omits an edge".into())
                    })?;
                if !tree_edges[edge_position] {
                    equation.insert(
                        chord_positions[edge_position],
                        boundary_sign(removed, modulus),
                    );
                }
            }
            Ok(equation)
        })
        .collect::<Result<Vec<_>>>()?;

    let chord_cocycles = nullspace(equations, chords.len(), modulus);
    chord_cocycles
        .into_iter()
        .map(|cocycle| {
            let mut expanded = SparseVector::default();
            for (&chord, &coefficient) in &cocycle.0 {
                let edge_position = chords.get(chord).copied().ok_or_else(|| {
                    Error::InvalidInput("cohomology forest gauge has an invalid chord".into())
                })?;
                expanded.insert(edge_position, coefficient);
            }
            Ok(expanded)
        })
        .collect()
}

fn edge_positions(
    vertex_count: usize,
    edges: &[Vec<usize>],
) -> Result<BTreeMap<(usize, usize), usize>> {
    let mut positions = BTreeMap::new();
    for (position, edge) in edges.iter().enumerate() {
        if edge.len() != 2 || edge[0] >= edge[1] || edge[1] >= vertex_count {
            return Err(Error::InvalidInput(
                "cohomology edge list is not canonically oriented".into(),
            ));
        }
        if positions.insert((edge[0], edge[1]), position).is_some() {
            return Err(Error::InvalidInput(
                "cohomology edge list contains a duplicate".into(),
            ));
        }
    }
    Ok(positions)
}

fn spanning_forest(
    vertex_count: usize,
    edge_positions: &BTreeMap<(usize, usize), usize>,
    edge_count: usize,
) -> Vec<bool> {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for (&(u, v), &position) in edge_positions {
        adjacency[u].push((v, position));
        adjacency[v].push((u, position));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }

    let mut visited = vec![false; vertex_count];
    let mut tree_edges = vec![false; edge_count];
    for root in 0..vertex_count {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(vertex) = queue.pop_front() {
            for &(neighbor, edge_position) in &adjacency[vertex] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    tree_edges[edge_position] = true;
                    queue.push_back(neighbor);
                }
            }
        }
    }
    tree_edges
}

fn validate_triangle(vertex_count: usize, triangle: &[usize]) -> Result<()> {
    if triangle.len() != 3
        || triangle[0] >= triangle[1]
        || triangle[1] >= triangle[2]
        || triangle[2] >= vertex_count
    {
        return Err(Error::InvalidInput(
            "cohomology triangle list is not canonically oriented".into(),
        ));
    }
    Ok(())
}

fn boundary_sign(position: usize, modulus: u64) -> u32 {
    if position % 2 == 0 {
        1
    } else {
        (modulus - 1) as u32
    }
}
