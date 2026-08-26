//! Adjacency bitsets with a rank index: one bit per pair at or below the
//! threshold, and the distance of every set bit reachable in constant time.
//!
//! The dim-0 apparent test asks, for an edge `(u, v)` of diameter `d`, for the
//! largest vertex `w` with `d(u, w) <= d` and `d(v, w) <= d`. With bitsets that
//! is a word-wise AND of two rows scanned from the top. Each candidate costs
//! two constant-time distance reads. The neighbor-list merge visits every entry
//! of both lists instead. The rows cost `n * n / 8` bytes plus half that for
//! the rank index. The engine builds them only when that is at most the size
//! of the graph itself.

use crate::distances::Distances;

/// The bytes of rows the engine accepts per edge of the graph. The sparse
/// graph itself costs about this much an edge, so the rows at most double
/// the memory of a run that has them.
pub(crate) const ADJACENCY_BYTES_PER_EDGE: usize = 64;

/// The fewest cycle edges (edges beyond a spanning forest) worth three
/// passes over the edges to build the rows.
pub(crate) const MIN_CYCLE_EDGES: usize = 1024;

/// One bit per ordered pair, plus a rank index and the distances in bit order.
pub(crate) struct Adjacency {
    words: usize,
    /// Row `u` is `bits[u * words..(u + 1) * words]`; bit `w` is set when
    /// `(u, w)` is an edge at or below the threshold.
    bits: Vec<u64>,
    /// `rank[u * words + k]` is the number of set bits of row `u` in words
    /// before `k`, so the position of neighbor `w` in `values` is
    /// `offset[u] + rank + popcount(word_k below w)`.
    rank: Vec<u32>,
    /// The distances of every row's neighbors, in ascending vertex order.
    values: Vec<f64>,
    offset: Vec<usize>,
}

impl Adjacency {
    /// The bytes an adjacency for `n` vertices and `edges` edges holds, with
    /// the activation rows the dim-0 walk adds: the bit rows and their rank
    /// index, the activation rows, the two distances of every edge, and the
    /// row offsets. `None` when the arithmetic overflows.
    pub(crate) fn bytes(n: usize, edges: usize) -> Option<usize> {
        let words = n.div_ceil(64);
        let rows = n.checked_mul(words)?.checked_mul(8 + 4 + 8)?;
        let values = edges.checked_mul(16)?;
        rows.checked_add(values)?.checked_add((n + 1) * 8)
    }

    /// Build the rows from every edge of `dist` at or below `threshold`, or
    /// `None` when the rows and the walk's activation rows together would
    /// take more than [`ADJACENCY_BYTES_PER_EDGE`] bytes an edge.
    pub(crate) fn build<D: Distances>(dist: &D, threshold: f64) -> Option<Self> {
        Self::build_gated(dist, threshold, MIN_CYCLE_EDGES)
    }

    /// [`Adjacency::build`] with the cycle-edge floor as a parameter, so a
    /// test can build rows for a small graph.
    pub(crate) fn build_gated<D: Distances>(
        dist: &D,
        threshold: f64,
        min_cycle_edges: usize,
    ) -> Option<Self> {
        let n = dist.len();
        let words = n.div_ceil(64);
        let mut edges = 0usize;
        dist.for_each_edge(|_, _, d| edges += usize::from(d <= threshold));
        // Too few cycle edges to test, or rows over the budget, or a row
        // whose rank would not fit its index: keep the neighbor lists.
        if edges.saturating_sub(n.saturating_sub(1)) < min_cycle_edges
            || Self::bytes(n, edges)? > edges.checked_mul(ADJACENCY_BYTES_PER_EDGE)?
            || u32::try_from(n).is_err()
        {
            return None;
        }
        let mut bits = vec![0u64; n * words];
        let mut degree = vec![0usize; n];
        dist.for_each_edge(|i, j, d| {
            if d <= threshold {
                bits[i * words + j / 64] |= 1 << (j % 64);
                bits[j * words + i / 64] |= 1 << (i % 64);
                degree[i] += 1;
                degree[j] += 1;
            }
        });
        let mut rank = vec![0u32; n * words];
        let mut offset = vec![0usize; n + 1];
        for u in 0..n {
            let mut acc = 0u32;
            for k in 0..words {
                rank[u * words + k] = acc;
                acc += bits[u * words + k].count_ones();
            }
            offset[u + 1] = offset[u] + degree[u];
        }
        let mut values = vec![0.0f64; offset[n]];
        let position = |u: usize, w: usize| -> usize {
            let k = w / 64;
            let below = bits[u * words + k] & ((1u64 << (w % 64)) - 1);
            offset[u] + rank[u * words + k] as usize + below.count_ones() as usize
        };
        dist.for_each_edge(|i, j, d| {
            if d <= threshold {
                values[position(i, j)] = d;
                values[position(j, i)] = d;
            }
        });
        Some(Self {
            words,
            bits,
            rank,
            values,
            offset,
        })
    }

    /// The distance of the edge `(u, w)`. The edge must be set.
    #[inline]
    pub(crate) fn get(&self, u: usize, w: usize) -> f64 {
        let k = u * self.words + w / 64;
        let below = self.bits[k] & ((1u64 << (w % 64)) - 1);
        self.values[self.offset[u] + self.rank[k] as usize + below.count_ones() as usize]
    }
}

/// Activation rows: one bit per ordered pair, set as the dim-0 walk passes
/// each edge. At a test, every edge at or below the diameter is set, and
/// possibly some above it.
pub(crate) struct Rows {
    words: usize,
    bits: Vec<u64>,
}

impl Rows {
    pub(crate) fn new(n: usize) -> Self {
        let words = n.div_ceil(64);
        Self {
            words,
            bits: vec![0u64; n * words],
        }
    }

    /// Set the edge `(u, v)` in both rows.
    #[inline]
    pub(crate) fn set(&mut self, u: usize, v: usize) {
        self.bits[u * self.words + v / 64] |= 1 << (v % 64);
        self.bits[v * self.words + u / 64] |= 1 << (u % 64);
    }

    /// True when the youngest cofacet of `(u, v)` with the edge's diameter
    /// pairs with the edge. The scan takes the largest common bit whose two
    /// edges, read from `adjacency`, are at or below the diameter. That bit
    /// is the cofacet's added vertex `w`. The facet check reuses the two
    /// distances. `u < v`.
    pub(crate) fn pairs_edge(&self, adjacency: &Adjacency, u: usize, v: usize, d: f64) -> bool {
        debug_assert!(u < v);
        let ru = &self.bits[u * self.words..(u + 1) * self.words];
        let rv = &self.bits[v * self.words..(v + 1) * self.words];
        for k in (0..self.words).rev() {
            let mut common = ru[k] & rv[k];
            while common != 0 {
                let bit = 63 - common.leading_zeros() as usize;
                let w = k * 64 + bit;
                let du = adjacency.get(u, w);
                let dv = adjacency.get(v, w);
                if du <= d && dv <= d {
                    return if w > v {
                        true
                    } else if w > u {
                        du < d
                    } else {
                        du < d && dv < d
                    };
                }
                common &= !(1u64 << bit);
            }
        }
        false
    }
}
