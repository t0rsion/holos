use crate::{Result, SparseDistanceMatrix};

use super::FactorizationSummary;

#[derive(Clone)]
pub(super) struct TerminalEdge {
    pub(super) u: usize,
    pub(super) v: usize,
    value: f64,
}

pub(super) struct Decomposition {
    pub(super) edges: Vec<TerminalEdge>,
    pub(super) blocks: Vec<Vec<usize>>,
    pub(super) summary: FactorizationSummary,
}

pub(super) fn decompose(matrix: &SparseDistanceMatrix, threshold: f64) -> Decomposition {
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

pub(super) fn block_is_cyclic(block: &[usize], edges: &[TerminalEdge]) -> bool {
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

pub(super) fn block_matrix(
    block: &[usize],
    edges: &[TerminalEdge],
) -> Result<SparseDistanceMatrix> {
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
