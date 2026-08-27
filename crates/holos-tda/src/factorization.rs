//! Vertex-biconnected factorization of sparse flag filtrations.
//!
//! The blocks are computed on the graph at the terminal level. Every clique
//! with at least two vertices lies in one vertex-biconnected block. The
//! positive-dimensional flag chain groups therefore split over those blocks.
//! H0 does not split at articulation vertices, so the engine computes it once
//! on the whole graph.

use rayon::prelude::*;
use std::collections::BTreeSet;

use crate::{
    Bar, Diagram, Error, GraphFactorization, Result, RipsParams, SparseDistanceMatrix, solver,
};

/// Structural counts for a terminal graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactorizationSummary {
    /// Vertex-biconnected blocks, including one-edge bridge blocks.
    pub blocks: usize,
    /// Blocks that can contain a graph cycle.
    pub cyclic_blocks: usize,
    /// Edges that are bridges at the terminal level.
    pub bridge_edges: usize,
    /// Edges in all cyclic blocks.
    pub cyclic_edges: usize,
    /// Edges in the largest cyclic block.
    pub largest_cyclic_block_edges: usize,
}

#[derive(Clone)]
struct TerminalEdge {
    u: usize,
    v: usize,
    value: f64,
}

struct Decomposition {
    edges: Vec<TerminalEdge>,
    blocks: Vec<Vec<usize>>,
    summary: FactorizationSummary,
}

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
    let positions: std::collections::BTreeMap<_, _> = block
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

/// Analyze the graph at `threshold` without running persistence.
///
/// `None` includes every listed edge. The same threshold rules apply as in
/// [`crate::rips_persistence_sparse`].
pub fn analyze(
    matrix: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<FactorizationSummary> {
    let threshold = checked_threshold(threshold)?;
    Ok(decompose(matrix, threshold).summary)
}

pub(crate) fn compute_sparse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<Diagram> {
    if params.max_dim == 0 || params.factorization == GraphFactorization::Off {
        return solver::compute(matrix, params);
    }
    let threshold = checked_threshold(params.threshold)?;
    let decomposition = decompose(matrix, threshold);
    if !selected(params.factorization, decomposition.summary) {
        return solver::compute(matrix, params);
    }

    let mut h0_params = params.clone();
    h0_params.max_dim = 0;
    h0_params.factorization = GraphFactorization::Off;
    let mut diagram = solver::compute(matrix, &h0_params)?;
    let cyclic: Vec<&[usize]> = decomposition
        .blocks
        .iter()
        .filter(|block| block_is_cyclic(block, &decomposition.edges))
        .map(Vec::as_slice)
        .collect();
    let parts = solve_cyclic_blocks(&cyclic, &decomposition.edges, params, threshold)?;
    for bars in parts {
        diagram.bars.extend(bars);
    }
    diagram.canonicalize();
    Ok(diagram)
}

fn solve_cyclic_blocks(
    blocks: &[&[usize]],
    edges: &[TerminalEdge],
    params: &RipsParams,
    threshold: f64,
) -> Result<Vec<Vec<Bar>>> {
    if params.threads > 1 && blocks.len() > 1 {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(params.threads)
            .build()
            .map_err(|error| Error::Io(format!("thread pool: {error}")))?;
        return pool.install(|| {
            blocks
                .par_iter()
                .map(|block| solve_block(block, edges, params, threshold, 1))
                .collect::<Result<Vec<_>>>()
        });
    }
    blocks
        .iter()
        .map(|block| solve_block(block, edges, params, threshold, params.threads))
        .collect()
}

fn solve_block(
    block: &[usize],
    edges: &[TerminalEdge],
    params: &RipsParams,
    threshold: f64,
    threads: usize,
) -> Result<Vec<Bar>> {
    let local = block_matrix(block, edges)?;
    let mut block_params = params.clone();
    block_params.collapse_edges = false;
    block_params.factorization = GraphFactorization::Off;
    block_params.threshold = Some(threshold);
    block_params.threads = threads;
    let block_diagram = solver::compute(&local, &block_params)?;
    Ok(block_diagram
        .bars
        .into_iter()
        .filter(|bar| bar.dim > 0)
        .collect())
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(Error::InvalidInput(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(if threshold == f64::INFINITY {
        f64::MAX
    } else {
        threshold
    })
}

fn selected(mode: GraphFactorization, summary: FactorizationSummary) -> bool {
    match mode {
        GraphFactorization::Off => false,
        GraphFactorization::Force => true,
        GraphFactorization::Auto => {
            summary.cyclic_blocks >= 2
                && 10u128 * summary.largest_cyclic_block_edges as u128
                    <= 9u128 * summary.cyclic_edges as u128
        }
    }
}

fn decompose(matrix: &SparseDistanceMatrix, threshold: f64) -> Decomposition {
    let edges: Vec<TerminalEdge> = matrix
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .map(|(u, v, value)| TerminalEdge { u, v, value })
        .collect();
    let mut adjacency = vec![Vec::<(usize, usize)>::new(); matrix.len()];
    for (edge, item) in edges.iter().enumerate() {
        adjacency[item.u].push((item.v, edge));
        adjacency[item.v].push((item.u, edge));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    let mut blocks = edge_blocks(&adjacency, &edges);
    for block in &mut blocks {
        block.sort_unstable();
    }
    blocks.sort_unstable();

    let mut cyclic_blocks = 0usize;
    let mut bridge_edges = 0usize;
    let mut cyclic_edges = 0usize;
    let mut largest = 0usize;
    for block in &blocks {
        if block_is_cyclic(block, &edges) {
            cyclic_blocks += 1;
            cyclic_edges += block.len();
            largest = largest.max(block.len());
        } else {
            debug_assert_eq!(block.len(), 1);
            bridge_edges += 1;
        }
    }
    let summary = FactorizationSummary {
        blocks: blocks.len(),
        cyclic_blocks,
        bridge_edges,
        cyclic_edges,
        largest_cyclic_block_edges: largest,
    };
    Decomposition {
        edges,
        blocks,
        summary,
    }
}

fn edge_blocks(adjacency: &[Vec<(usize, usize)>], edges: &[TerminalEdge]) -> Vec<Vec<usize>> {
    BlockSearch::new(adjacency, edges).run()
}

struct BlockSearch<'a> {
    adjacency: &'a [Vec<(usize, usize)>],
    edges: &'a [TerminalEdge],
    discovered: Vec<usize>,
    low: Vec<usize>,
    next: Vec<usize>,
    parent_edge: Vec<usize>,
    time: usize,
    path: Vec<usize>,
    edge_stack: Vec<usize>,
    blocks: Vec<Vec<usize>>,
}

impl<'a> BlockSearch<'a> {
    fn new(adjacency: &'a [Vec<(usize, usize)>], edges: &'a [TerminalEdge]) -> Self {
        let count = adjacency.len();
        Self {
            adjacency,
            edges,
            discovered: vec![usize::MAX; count],
            low: vec![0; count],
            next: vec![0; count],
            parent_edge: vec![usize::MAX; count],
            time: 0,
            path: Vec::new(),
            edge_stack: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Vec<usize>> {
        for root in 0..self.adjacency.len() {
            if self.discovered[root] == usize::MAX && !self.adjacency[root].is_empty() {
                self.start_root(root);
                self.walk_root();
            }
        }
        self.blocks
    }

    fn start_root(&mut self, root: usize) {
        self.discovered[root] = self.time;
        self.low[root] = self.time;
        self.time += 1;
        self.path.push(root);
    }

    fn walk_root(&mut self) {
        while let Some(&vertex) = self.path.last() {
            if let Some((neighbor, edge)) = self.next_neighbor(vertex) {
                self.visit_edge(vertex, neighbor, edge);
            } else {
                self.finish_vertex(vertex);
            }
        }
    }

    fn next_neighbor(&mut self, vertex: usize) -> Option<(usize, usize)> {
        let neighbor = self.adjacency[vertex].get(self.next[vertex]).copied();
        self.next[vertex] += usize::from(neighbor.is_some());
        neighbor
    }

    fn visit_edge(&mut self, vertex: usize, neighbor: usize, edge: usize) {
        if self.discovered[neighbor] == usize::MAX {
            self.parent_edge[neighbor] = edge;
            self.edge_stack.push(edge);
            self.discovered[neighbor] = self.time;
            self.low[neighbor] = self.time;
            self.time += 1;
            self.path.push(neighbor);
        } else if edge != self.parent_edge[vertex]
            && self.discovered[neighbor] < self.discovered[vertex]
        {
            self.low[vertex] = self.low[vertex].min(self.discovered[neighbor]);
            self.edge_stack.push(edge);
        }
    }

    fn finish_vertex(&mut self, vertex: usize) {
        self.path.pop();
        let edge = self.parent_edge[vertex];
        if edge == usize::MAX {
            debug_assert!(self.edge_stack.is_empty());
            return;
        }
        let item = &self.edges[edge];
        let parent = if item.u == vertex { item.v } else { item.u };
        self.low[parent] = self.low[parent].min(self.low[vertex]);
        if self.low[vertex] >= self.discovered[parent] {
            let block = self.pop_block(edge);
            self.blocks.push(block);
        }
    }

    fn pop_block(&mut self, terminal_edge: usize) -> Vec<usize> {
        let mut block = Vec::new();
        loop {
            let edge = self
                .edge_stack
                .pop()
                .expect("tree edge is on the block stack");
            block.push(edge);
            if edge == terminal_edge {
                return block;
            }
        }
    }
}

fn block_is_cyclic(block: &[usize], edges: &[TerminalEdge]) -> bool {
    if block.len() < 3 {
        return false;
    }
    let mut vertices = Vec::with_capacity(block.len() + 1);
    for &edge in block {
        vertices.push(edges[edge].u);
        vertices.push(edges[edge].v);
    }
    vertices.sort_unstable();
    vertices.dedup();
    block.len() >= vertices.len()
}

fn block_matrix(block: &[usize], edges: &[TerminalEdge]) -> Result<SparseDistanceMatrix> {
    let mut vertices = Vec::with_capacity(block.len() + 1);
    for &edge in block {
        vertices.push(edges[edge].u);
        vertices.push(edges[edge].v);
    }
    vertices.sort_unstable();
    vertices.dedup();
    let mut triplets = Vec::with_capacity(block.len());
    for &edge in block {
        let edge = &edges[edge];
        let u = vertices
            .binary_search(&edge.u)
            .expect("block contains its edge endpoint");
        let v = vertices
            .binary_search(&edge.v)
            .expect("block contains its edge endpoint");
        triplets.push((u, v, edge.value));
    }
    SparseDistanceMatrix::from_triplets(vertices.len(), &triplets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RipsParams, rips_persistence_sparse};

    #[test]
    fn factorization_is_off_by_default() {
        assert_eq!(RipsParams::default().factorization, GraphFactorization::Off);
    }

    fn graph(n: usize, pairs: &[(usize, usize)]) -> SparseDistanceMatrix {
        let edges: Vec<_> = pairs
            .iter()
            .enumerate()
            .map(|(i, &(u, v))| (u, v, 1.0 + (i % 3) as f64))
            .collect();
        SparseDistanceMatrix::from_triplets(n, &edges).unwrap()
    }

    #[test]
    fn blocks_partition_edges_and_classify_bridges() {
        let matrix = graph(
            8,
            &[
                (0, 1),
                (1, 2),
                (2, 0),
                (2, 3),
                (3, 4),
                (4, 2),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 5),
            ],
        );
        let summary = analyze(&matrix, None).unwrap();
        assert_eq!(summary.blocks, 4);
        assert_eq!(summary.cyclic_blocks, 3);
        assert_eq!(summary.bridge_edges, 1);
        assert_eq!(summary.cyclic_edges, 9);
        assert_eq!(summary.largest_cyclic_block_edges, 3);
    }

    #[test]
    fn forced_factorization_matches_whole_graph_over_fields_and_threads() {
        let mut pairs = Vec::new();
        for offset in [0usize, 4, 8] {
            pairs.extend([
                (offset, offset + 1),
                (offset + 1, offset + 2),
                (offset + 2, offset + 3),
                (offset, offset + 3),
            ]);
        }
        pairs.extend([(3, 4), (7, 8)]);
        let matrix = graph(12, &pairs);
        for modulus in [2, 3, 5] {
            for threads in [1, 3] {
                let mut whole = RipsParams::new(2).with_modulus(modulus);
                whole.threads = threads;
                whole.factorization = GraphFactorization::Off;
                let mut split = whole.clone();
                split.factorization = GraphFactorization::Force;
                assert_eq!(
                    rips_persistence_sparse(&matrix, &whole).unwrap().bars,
                    rips_persistence_sparse(&matrix, &split).unwrap().bars,
                    "modulus {modulus}, threads {threads}"
                );
            }
        }
    }

    #[test]
    fn a_deep_path_does_not_recurse() {
        let pairs: Vec<_> = (0..20_000).map(|u| (u, u + 1)).collect();
        let matrix = graph(20_001, &pairs);
        let summary = analyze(&matrix, None).unwrap();
        assert_eq!(summary.blocks, 20_000);
        assert_eq!(summary.bridge_edges, 20_000);
        assert_eq!(summary.cyclic_blocks, 0);
    }

    #[test]
    fn random_graphs_match_whole_reduction_and_keep_cliques_in_one_block() {
        let mut state = 0x82af_137c_d095_4e61u64;
        for case in 0..80 {
            let matrix = random_graph(&mut state);
            for threshold in [1.0, 3.0] {
                check_random_decomposition(&matrix, case, threshold);
                check_random_persistence(&matrix, case, threshold);
            }
        }
    }

    fn random_graph(state: &mut u64) -> SparseDistanceMatrix {
        let n = 5 + next_random(state) as usize % 10;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next_random(state) % 5 < 2 {
                    triplets.push((u, v, (next_random(state) % 4) as f64));
                }
            }
        }
        SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn check_random_decomposition(matrix: &SparseDistanceMatrix, case: usize, threshold: f64) {
        let decomposition = decompose(matrix, threshold);
        let mut owner = vec![usize::MAX; decomposition.edges.len()];
        for (block, edge_ids) in decomposition.blocks.iter().enumerate() {
            for &edge in edge_ids {
                assert_eq!(owner[edge], usize::MAX, "case {case}: repeated edge");
                owner[edge] = block;
            }
        }
        assert!(owner.iter().all(|&block| block != usize::MAX));
        check_triangle_owners(matrix.len(), &decomposition, &owner, case);
    }

    fn check_triangle_owners(
        vertices: usize,
        decomposition: &Decomposition,
        owner: &[usize],
        case: usize,
    ) {
        for a in 0..vertices {
            for b in a + 1..vertices {
                for c in b + 1..vertices {
                    check_triangle_owner(decomposition, owner, [a, b, c], case);
                }
            }
        }
    }

    fn check_triangle_owner(
        decomposition: &Decomposition,
        owner: &[usize],
        vertices: [usize; 3],
        case: usize,
    ) {
        let edge = |u: usize, v: usize| {
            decomposition
                .edges
                .iter()
                .position(|item| item.u == u && item.v == v)
        };
        let [a, b, c] = vertices;
        if let (Some(ab), Some(ac), Some(bc)) = (edge(a, b), edge(a, c), edge(b, c)) {
            assert_eq!(owner[ab], owner[ac], "case {case}: triangle");
            assert_eq!(owner[ab], owner[bc], "case {case}: triangle");
        }
    }

    fn check_random_persistence(matrix: &SparseDistanceMatrix, case: usize, threshold: f64) {
        for modulus in [2, 3] {
            let mut whole = RipsParams::new(2)
                .with_modulus(modulus)
                .with_threshold(threshold);
            whole.factorization = GraphFactorization::Off;
            let mut split = whole.clone();
            split.factorization = GraphFactorization::Force;
            assert_eq!(
                rips_persistence_sparse(matrix, &whole).unwrap().bars,
                rips_persistence_sparse(matrix, &split).unwrap().bars,
                "case {case}, threshold {threshold}, modulus {modulus}"
            );
        }
    }
}
