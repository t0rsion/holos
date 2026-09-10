//! Vertex-biconnected factorization of sparse flag filtrations.
//!
//! The blocks are computed on the terminal graph. Every clique with at
//! least two vertices lies in one vertex-biconnected block. The
//! positive-dimensional flag chain groups therefore split over those blocks.
//! H0 does not split at articulation vertices, so the engine computes it once
//! on the whole graph.

mod decomposition;
mod program;

#[cfg(test)]
mod tests;

use rayon::prelude::*;

use crate::{
    Bar, Diagram, Error, GraphFactorization, Result, RipsParams, SparseDistanceMatrix, solver,
};

use decomposition::{TerminalEdge, block_is_cyclic, block_matrix, decompose};
pub(crate) use program::{ProgramBlock, ProgramDecompositionSummary, program_blocks};

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
