use std::collections::{BTreeMap, BTreeSet};

use crate::ProofError;

use super::diagram::checked_threshold;
use super::model::{Block, Graph};

pub(crate) fn program_blocks(
    graph: &Graph,
    threshold: Option<f64>,
) -> Result<Vec<Block>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    let active: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .copied()
        .collect();
    let mut adjacency = vec![Vec::<(usize, usize)>::new(); graph.vertex_count];
    for (position, edge) in active.iter().enumerate() {
        adjacency[edge.u].push((edge.v, position));
        adjacency[edge.v].push((edge.u, position));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    let edge_blocks = biconnected_edge_blocks(&adjacency);
    let mut initial = Vec::new();
    for positions in edge_blocks {
        let mut vertices = Vec::new();
        let mut edges = Vec::new();
        for position in positions {
            let edge = active[position];
            vertices.push(edge.u);
            vertices.push(edge.v);
            edges.push([edge.u, edge.v]);
        }
        vertices.sort_unstable();
        vertices.dedup();
        edges.sort_unstable();
        initial.push(Block { vertices, edges });
    }
    let mut search = SeparatorSearch {
        graph,
        checked: 0,
        complete: true,
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
    Ok(blocks)
}

fn biconnected_edge_blocks(adjacency: &[Vec<(usize, usize)>]) -> Vec<Vec<usize>> {
    BiconnectedSearch::new(adjacency).run()
}

struct BiconnectedSearch<'a> {
    adjacency: &'a [Vec<(usize, usize)>],
    discovered: Vec<usize>,
    low: Vec<usize>,
    next: Vec<usize>,
    parent_edge: Vec<usize>,
    time: usize,
    path: Vec<usize>,
    edge_stack: Vec<usize>,
    blocks: Vec<Vec<usize>>,
}

impl<'a> BiconnectedSearch<'a> {
    fn new(adjacency: &'a [Vec<(usize, usize)>]) -> Self {
        let vertices = adjacency.len();
        Self {
            adjacency,
            discovered: vec![usize::MAX; vertices],
            low: vec![0; vertices],
            next: vec![0; vertices],
            parent_edge: vec![usize::MAX; vertices],
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
                self.walk();
            }
        }
        for block in &mut self.blocks {
            block.sort_unstable();
        }
        self.blocks.sort_unstable();
        self.blocks
    }

    fn start_root(&mut self, root: usize) {
        self.discovered[root] = self.time;
        self.low[root] = self.time;
        self.time += 1;
        self.path.push(root);
    }

    fn walk(&mut self) {
        while let Some(&vertex) = self.path.last() {
            if self.next[vertex] < self.adjacency[vertex].len() {
                let (neighbor, edge) = self.adjacency[vertex][self.next[vertex]];
                self.next[vertex] += 1;
                self.visit_edge(vertex, neighbor, edge);
            } else {
                self.path.pop();
                self.finish_vertex(vertex);
            }
        }
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
        let edge = self.parent_edge[vertex];
        if edge == usize::MAX {
            if !self.edge_stack.is_empty() {
                self.blocks.push(std::mem::take(&mut self.edge_stack));
            }
            return;
        }
        let parent = self.adjacency[vertex]
            .iter()
            .find_map(|&(neighbor, candidate)| (candidate == edge).then_some(neighbor))
            .expect("a tree edge has its parent endpoint");
        self.low[parent] = self.low[parent].min(self.low[vertex]);
        if self.low[vertex] >= self.discovered[parent] {
            let block = self.pop_block(edge);
            self.blocks.push(block);
        }
    }

    fn pop_block(&mut self, terminal: usize) -> Vec<usize> {
        let mut block = Vec::new();
        while let Some(candidate) = self.edge_stack.pop() {
            block.push(candidate);
            if candidate == terminal {
                break;
            }
        }
        block
    }
}

struct SeparatorSearch<'a> {
    graph: &'a Graph,
    checked: usize,
    complete: bool,
}

impl SeparatorSearch<'_> {
    fn refine(&mut self, block: Block, output: &mut Vec<Block>) {
        if !self.complete || block.edges.len() < block.vertices.len() {
            output.push(block);
            return;
        }
        let Some((separator, components)) = self.find(&block) else {
            output.push(block);
            return;
        };
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
            self.refine(Block { vertices, edges }, output);
        }
    }

    fn find(&mut self, block: &Block) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let maximum = crate::SEPARATOR_WIDTH.min(block.vertices.len().saturating_sub(2));
        for width in 2..=maximum {
            let mut positions: Vec<_> = (0..width).collect();
            loop {
                if self.checked == crate::SEPARATOR_SEARCH_LIMIT {
                    self.complete = false;
                    return None;
                }
                self.checked += 1;
                let separator: Vec<_> = positions
                    .iter()
                    .map(|&position| block.vertices[position])
                    .collect();
                if zero_simplex(self.graph, &separator) {
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
}

fn zero_simplex(graph: &Graph, vertices: &[usize]) -> bool {
    for (position, &u) in vertices.iter().enumerate() {
        for &v in &vertices[position + 1..] {
            if graph.get(u, v).to_bits() != 0 {
                return false;
            }
        }
    }
    true
}

fn components_without(block: &Block, separator: &[usize]) -> Vec<Vec<usize>> {
    let excluded: BTreeSet<_> = separator.iter().copied().collect();
    let positions: BTreeMap<_, _> = block
        .vertices
        .iter()
        .copied()
        .enumerate()
        .map(|(position, vertex)| (vertex, position))
        .collect();
    let mut adjacency = vec![Vec::new(); block.vertices.len()];
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
