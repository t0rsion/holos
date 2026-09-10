use std::collections::{BTreeMap, BTreeSet};

use crate::{Result, SparseDistanceMatrix};

use super::decomposition::decompose;
use super::{FactorizationSummary, checked_threshold};

#[derive(Debug, Clone)]
pub(crate) struct ProgramBlock {
    pub(crate) vertices: Vec<usize>,
    pub(crate) edges: Vec<[usize; 2]>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProgramDecompositionSummary {
    pub(crate) articulation: FactorizationSummary,
    pub(crate) articulation_vertices: usize,
    pub(crate) zero_simplex_separators: usize,
    pub(crate) widest_separator: usize,
    pub(crate) separator_candidates_checked: usize,
    pub(crate) separator_search_complete: bool,
}

const PROGRAM_SEPARATOR_WIDTH: usize = 3;
const PROGRAM_SEPARATOR_SEARCH_LIMIT: usize = 100_000;

pub(crate) fn program_blocks(
    matrix: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<(ProgramDecompositionSummary, Vec<ProgramBlock>)> {
    let threshold = checked_threshold(threshold)?;
    let decomposition = decompose(matrix, threshold);
    let initial: Vec<_> = decomposition
        .blocks
        .iter()
        .map(|block| {
            let mut vertices = Vec::with_capacity(block.len() + 1);
            let mut edges = Vec::with_capacity(block.len());
            for &edge in block {
                let edge = &decomposition.edges[edge];
                vertices.push(edge.u);
                vertices.push(edge.v);
                edges.push([edge.u, edge.v]);
            }
            vertices.sort_unstable();
            vertices.dedup();
            edges.sort_unstable();
            ProgramBlock { vertices, edges }
        })
        .collect();
    let mut counts = vec![0usize; matrix.len()];
    for block in &initial {
        for &vertex in &block.vertices {
            counts[vertex] += 1;
        }
    }
    let articulation_vertices = counts.iter().filter(|&&count| count > 1).count();
    let mut search = SeparatorSearch {
        matrix,
        checked: 0,
        complete: true,
        separators: 0,
        widest: 1,
    };
    let mut blocks = Vec::new();
    for block in initial {
        search.refine(block, &mut blocks);
    }
    blocks.sort_by(|left, right| {
        left.vertices
            .cmp(&right.vertices)
            .then(left.edges.cmp(&right.edges))
    });
    Ok((
        ProgramDecompositionSummary {
            articulation: decomposition.summary,
            articulation_vertices,
            zero_simplex_separators: search.separators,
            widest_separator: search.widest,
            separator_candidates_checked: search.checked,
            separator_search_complete: search.complete,
        },
        blocks,
    ))
}

struct SeparatorSearch<'a> {
    matrix: &'a SparseDistanceMatrix,
    checked: usize,
    complete: bool,
    separators: usize,
    widest: usize,
}

impl SeparatorSearch<'_> {
    fn refine(&mut self, block: ProgramBlock, output: &mut Vec<ProgramBlock>) {
        if !self.complete || block.edges.len() < block.vertices.len() {
            output.push(block);
            return;
        }
        let Some((separator, components)) = self.find(&block) else {
            output.push(block);
            return;
        };
        self.separators += 1;
        self.widest = self.widest.max(separator.len());
        for component in components {
            let mut vertices = separator.clone();
            vertices.extend(component);
            vertices.sort_unstable();
            let members: BTreeSet<_> = vertices.iter().copied().collect();
            let edges = block
                .edges
                .iter()
                .copied()
                .filter(|[u, v]| members.contains(u) && members.contains(v))
                .collect();
            self.refine(ProgramBlock { vertices, edges }, output);
        }
    }

    fn find(&mut self, block: &ProgramBlock) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let max_width = PROGRAM_SEPARATOR_WIDTH.min(block.vertices.len().saturating_sub(2));
        for width in 2..=max_width {
            let mut positions: Vec<_> = (0..width).collect();
            loop {
                if self.checked == PROGRAM_SEPARATOR_SEARCH_LIMIT {
                    self.complete = false;
                    return None;
                }
                self.checked += 1;
                let separator: Vec<_> = positions
                    .iter()
                    .map(|&position| block.vertices[position])
                    .collect();
                if self.is_zero_simplex(&separator) {
                    let components = components_without(block, &separator);
                    if components.len() > 1 {
                        return Some((separator, components));
                    }
                }
                if !next_combination(&mut positions, block.vertices.len()) {
                    break;
                }
            }
        }
        None
    }

    fn is_zero_simplex(&self, vertices: &[usize]) -> bool {
        for (position, &u) in vertices.iter().enumerate() {
            for &v in &vertices[position + 1..] {
                if self.matrix.get(u, v).to_bits() != 0 {
                    return false;
                }
            }
        }
        true
    }
}

fn components_without(block: &ProgramBlock, separator: &[usize]) -> Vec<Vec<usize>> {
    let excluded: BTreeSet<_> = separator.iter().copied().collect();
    let mut adjacency = vec![Vec::new(); block.vertices.len()];
    let positions: BTreeMap<_, _> = block
        .vertices
        .iter()
        .copied()
        .enumerate()
        .map(|(position, vertex)| (vertex, position))
        .collect();
    for &[u, v] in &block.edges {
        if excluded.contains(&u) || excluded.contains(&v) {
            continue;
        }
        adjacency[positions[&u]].push(v);
        adjacency[positions[&v]].push(u);
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for &root in &block.vertices {
        if excluded.contains(&root) || !seen.insert(root) {
            continue;
        }
        let mut stack = vec![root];
        let mut component = Vec::new();
        while let Some(vertex) = stack.pop() {
            component.push(vertex);
            for &neighbor in &adjacency[positions[&vertex]] {
                if seen.insert(neighbor) {
                    stack.push(neighbor);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components.sort_unstable();
    components
}

fn next_combination(positions: &mut [usize], universe: usize) -> bool {
    for index in (0..positions.len()).rev() {
        let maximum = universe - (positions.len() - index);
        if positions[index] < maximum {
            positions[index] += 1;
            for next in index + 1..positions.len() {
                positions[next] = positions[next - 1] + 1;
            }
            return true;
        }
    }
    false
}
