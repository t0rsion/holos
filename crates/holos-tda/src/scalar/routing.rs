use super::model::{DenseStorage, RipsParams};
use crate::DistanceMatrix;

/// Smallest point count [`Engine::Auto`] routes. The conversion costs one
/// pass over the matrix, and a short reduction cannot earn that back.
pub(super) const N_MIN: usize = 32;

/// Numerator of the highest edge density [`Engine::Auto`] routes:
/// `m / C(n, 2)` at the resolved threshold, with `m` the finite pairs at or
/// below it. The cutoff is four fifths, and [`density_routes`] applies it
/// as `5 * m <= 4 * C(n, 2)` so that no rounding of `0.8` and no rounding
/// of a large count enters the decision.
///
/// Above the cutoff the graph would only spend memory on edges the matrix
/// already holds.
pub(super) const RHO_MAX_NUM: u128 = 4;

/// Denominator of [`RHO_MAX_NUM`].
pub(super) const RHO_MAX_DEN: u128 = 5;

/// The threshold a dense run applies: the caller's, or the enclosing
/// radius.
pub(super) fn resolved_threshold(dist: &DistanceMatrix, params: &RipsParams) -> f64 {
    params.threshold.unwrap_or_else(|| dist.enclosing_radius())
}

/// True when a dense input can route at all. Below [`N_MIN`] points the
/// answer is dense whatever the matrix holds, so the counting pass never
/// runs. A negative threshold is an error the engine reports, and a NaN
/// threshold fails the comparison, so neither routes.
///
/// An infinite threshold does route. It admits every finite pair and no
/// absent one, and a matrix of mostly absent pairs has a sparse graph at
/// that threshold, so the density cutoff is what decides. A matrix with no
/// absent pair has every pair as an edge and fails the cutoff.
pub(super) fn may_route(n: usize, threshold: f64) -> bool {
    n >= N_MIN && threshold >= 0.0
}

/// True when the edge density at the threshold is at or below the frozen
/// cutoff. `edges` is the count at the same threshold.
///
/// A dense matrix stores every pair, so `C(n, 2)` is bounded by the address
/// space and both products fit u128 with room to spare.
pub(super) fn density_routes(n: usize, edges: usize) -> bool {
    RHO_MAX_DEN * edges as u128 <= RHO_MAX_NUM * pair_count(n)
}

/// Bytes the conversion holds per retained edge at its peak.
///
/// `DistanceMatrix::to_sparse_at` writes both directed entries of an edge
/// into one index array (`u32`, 4 bytes each) and one value array (`f64`,
/// 8 bytes each), so an edge costs 24 bytes. No triplet buffer exists.
pub(super) const CONVERSION_BYTES_PER_EDGE: u128 = 24;

/// Bytes the conversion holds per point at its peak: the degree count, the
/// row offset, and the fill cursor, 8 bytes each. One more offset closes
/// the last row.
pub(super) const CONVERSION_BYTES_PER_POINT: u128 = 24;

/// Smallest conversion budget [`Engine::Auto`] grants, in bytes. A small
/// matrix is a few kilobytes, too little for any useful graph, so the
/// budget never falls below this.
pub(super) const MIN_CONVERSION_BYTES: u128 = 32 * 1024 * 1024;

/// The pairs of `n` points, `C(n, 2)`, wide enough not to overflow.
pub(super) fn pair_count(n: usize) -> u128 {
    let n = n as u128;
    n * n.saturating_sub(1) / 2
}

/// True when the conversion peak fits the budget: [`MIN_CONVERSION_BYTES`],
/// or the bytes of the compact matrix (8 per pair), whichever is larger.
/// `edges` is the count at the resolved threshold.
///
/// The density cutoff bounds the graph in proportion to the matrix, and
/// this bounds it in bytes: a routed run then peaks at about twice the
/// compact matrix, below the dense engine's peak in the square form.
/// [`Engine::Sparse`] is an explicit request and ignores the budget.
pub(super) fn memory_routes(n: usize, edges: usize) -> bool {
    let extra =
        CONVERSION_BYTES_PER_EDGE * edges as u128 + CONVERSION_BYTES_PER_POINT * n as u128 + 8;
    let budget = MIN_CONVERSION_BYTES.max(pair_count(n) * 8);
    extra <= budget
}

/// True when the counted graph is worth building: its density is at or
/// below the cutoff and its conversion fits the budget.
pub(super) fn graph_routes(n: usize, edges: usize) -> bool {
    density_routes(n, edges) && memory_routes(n, edges)
}

/// Smallest compact matrix [`DenseStorage::Auto`] converts, in bytes. The
/// bound is 1025 points.
///
/// The full form doubles the matrix. Below this size both the matrix and
/// the reduction are small in absolute terms, so `Auto` keeps the compact
/// form and leaves the second triangle to a caller who asks for it.
///
/// Frozen on 2026-08-18 from disclosed engineering data.
pub(super) const SQUARE_MIN_BYTES: u128 = 4 << 20;

/// Most bytes [`DenseStorage::Auto`] adds for the full form. The full form
/// adds `n(n+1)/2` entries, the compact matrix again plus its diagonal, so
/// this bounds the point count as well: 8191 points.
///
/// The bound caps what a run spends on a storage form nobody asked for.
/// [`DenseStorage::Square`] is an explicit request and ignores it.
pub(super) const SQUARE_EXTRA_MAX_BYTES: u128 = 256 << 20;

/// Distance reads per matrix cell the fold must make before
/// [`DenseStorage::Auto`] converts. The conversion writes `n * n` cells,
/// and those reads are what the full form makes contiguous, so the ratio
/// is what the conversion has to earn back. The dim-0 columns supply one
/// read per cell on their own, so the test asks the columns above them for
/// three more.
pub(super) const SQUARE_READS_PER_CELL: u128 = 4;

/// Bytes the full form adds over the compact one: the entries above the
/// diagonal and the diagonal itself.
pub(super) fn square_extra_bytes(n: usize) -> u128 {
    let n = n as u128;
    n * (n + 1) / 2 * 8
}

/// Distances the cofacet diameter fold reads, as the rule estimates them.
///
/// Two terms. The dim-0 columns classify the cofacets of every vertex, so
/// they read the whole matrix once: `n * n`. Each dim-1 column then walks
/// the candidate range and reads one distance per candidate, so the dim-1
/// columns read `edges * n` between them, and from `max_dim` 2 up the
/// assembly enumerates the same cofacets once more per further dimension.
pub(super) fn fold_reads(n: usize, edges: usize, max_dim: usize) -> u128 {
    let n = n as u128;
    n * n + edges as u128 * n * max_dim.max(1) as u128
}

/// True when the matrix is large enough for the full form to pay and its
/// added bytes fit the budget.
pub(super) fn square_size_fits(n: usize) -> bool {
    pair_count(n) * 8 >= SQUARE_MIN_BYTES && square_extra_bytes(n) <= SQUARE_EXTRA_MAX_BYTES
}

/// True when the fold reads enough distances to earn the conversion.
pub(super) fn square_work_pays(n: usize, edges: usize, max_dim: usize) -> bool {
    let cells = n as u128 * n as u128;
    fold_reads(n, edges, max_dim) >= SQUARE_READS_PER_CELL * cells
}

/// True when the dense run reduces from the full form. `edges` is the
/// count at `threshold` when the caller has already made it; the rule
/// counts for itself only when the size test has already passed, so a run
/// that cannot convert never pays for a pass.
pub(super) fn square_selected(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
    edges: Option<usize>,
) -> bool {
    match params.dense_storage {
        DenseStorage::Compact => false,
        DenseStorage::Square => true,
        DenseStorage::Auto => {
            let n = dist.len();
            if !square_size_fits(n) {
                return false;
            }
            let edges = edges.unwrap_or_else(|| dist.count_edges_at(threshold));
            square_work_pays(n, edges, params.max_dim)
        }
    }
}
