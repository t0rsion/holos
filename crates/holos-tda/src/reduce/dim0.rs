use rayon::prelude::*;

use crate::budget::Region;
use crate::distances::Distances;
use crate::field::Coeffs;
use crate::simplex::Simplex;
use crate::union_find::UnionFind;
use crate::{Bar, Diagram};

use super::model::Dim0Walk;
use super::{Engine, PairScratch, Pairing};

const DIM0_ROWS_BLOCK: usize = 8192;
const DIM0_ROWS_BLOCK_SERIAL: usize = 512;

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
    /// Emit the dim-0 bars with one union-find pass and return the dim-1
    /// columns (cycle edges), sorted for reduction (diameter descending,
    /// index ascending).
    ///
    /// The pass has four steps: sort the edges once, walk them in that order
    /// with the union-find, test each cycle edge for a zero-apparent cofacet,
    /// and keep the accepted edges in the walk order. The walk stays serial,
    /// because its result depends on the order. On one thread the test runs
    /// inside the walk. With workers it runs after the walk, on the pool.
    pub(super) fn dim0_pairs(&self, edges: &[Simplex], diagram: &mut Diagram) -> Vec<Simplex> {
        let sorted = self.sorted_edge_keys(edges);
        let cycle_bound = edges.len().saturating_sub(self.n.saturating_sub(1));
        if let Some(adjacency) = &self.adjacency {
            return self.dim0_pairs_by_rows(&sorted, adjacency, diagram);
        }
        let defer = self.workers(Region::Prefilter, cycle_bound) > 1;
        let mut walk = self.walk_dim0_edges(&sorted, defer, diagram);
        self.emit_essential_h0(&mut walk.union_find, diagram);
        if defer {
            walk.columns = self.dim0_columns(walk.cycles);
        }
        walk.columns.reverse();
        walk.columns
    }

    pub(super) fn sorted_edge_keys(&self, edges: &[Simplex]) -> Vec<u128> {
        let mut sorted: Vec<_> = edges.iter().map(|&edge| edge_key(edge)).collect();
        match &self.pool {
            Some(pool) if self.workers(Region::Sort, sorted.len()) > 1 => {
                pool.install(|| sorted.par_sort_unstable())
            }
            _ => sorted.sort_unstable(),
        }
        sorted
    }

    pub(super) fn walk_dim0_edges(
        &self,
        sorted: &[u128],
        defer: bool,
        diagram: &mut Diagram,
    ) -> Dim0Walk {
        let mut uf = UnionFind::new(self.n);
        let mut cycles: Vec<(Simplex, [usize; 2])> = Vec::new();
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        let mut pairs = PairScratch::default();
        for &key in sorted {
            let e = edge_from_key(key);
            self.bt.unrank(e.index, 1, self.n, &mut verts);
            let (ru, rv) = (uf.find(verts[0]), uf.find(verts[1]));
            if ru != rv {
                emit_dim0_pair(e, diagram);
                uf.link(ru, rv);
            } else if self.max_dim > 0 {
                if defer {
                    // Carry the vertices, so the deferred test does not
                    // decode the edge a second time.
                    cycles.push((e, [verts[0], verts[1]]));
                } else if self.is_dim0_column(&verts, e, &mut pairs) {
                    columns.push(e);
                }
            }
        }
        Dim0Walk {
            union_find: uf,
            cycles,
            columns,
        }
    }

    pub(super) fn emit_essential_h0(&self, union_find: &mut UnionFind, diagram: &mut Diagram) {
        for vertex in 0..self.n {
            if union_find.find(vertex) == vertex {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                });
            }
        }
    }

    /// The dim-0 walk with the apparent test on activation rows. The walk
    /// takes the sorted edges in blocks closed at a diameter boundary: it
    /// first sets the bit of every edge of the block in both ends' rows, then
    /// it walks the block. Rows may also hold later edges in the block; the
    /// test checks each candidate against the diameter under test. For a
    /// cycle edge `(u, v)` the largest common bit of the two rows with both
    /// other edges at or below the diameter is the youngest cofacet of equal
    /// diameter. The facet check reads at most two distances from
    /// `adjacency`. The columns and their order are those of the plain walk.
    pub(super) fn dim0_pairs_by_rows(
        &self,
        sorted: &[u128],
        adjacency: &crate::adjacency::Adjacency,
        diagram: &mut Diagram,
    ) -> Vec<Simplex> {
        let cycle_bound = sorted.len().saturating_sub(self.n.saturating_sub(1));
        let workers = self.workers(Region::Prefilter, cycle_bound);
        // Serial blocks are small: the walk pays a loop per block, and a
        // large block gives the serial test more candidates above the
        // diameter to reject.
        let block = if workers > 1 {
            DIM0_ROWS_BLOCK
        } else {
            DIM0_ROWS_BLOCK_SERIAL
        };
        let mut rows = crate::adjacency::Rows::new(self.n);
        let mut uf = UnionFind::new(self.n);
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        let mut ends: Vec<[usize; 2]> = Vec::new();
        let mut cycles: Vec<(Simplex, [usize; 2])> = Vec::new();
        let mut keep: Vec<u8> = Vec::new();
        let mut start = 0;
        while start < sorted.len() {
            let end = row_block_end(sorted, start, block);
            self.decode_dim0_ends(&sorted[start..end], workers, &mut ends, &mut verts);
            for uv in &ends {
                rows.set(uv[0], uv[1]);
            }
            cycles.clear();
            self.walk_dim0_row_block(
                &sorted[start..end],
                &ends,
                workers,
                adjacency,
                &rows,
                &mut uf,
                diagram,
                &mut cycles,
                &mut columns,
            );
            self.filter_row_cycles(adjacency, &rows, workers, &cycles, &mut keep, &mut columns);
            start = end;
        }
        self.emit_essential_h0(&mut uf, diagram);
        columns.reverse();
        columns
    }

    pub(super) fn decode_dim0_ends(
        &self,
        keys: &[u128],
        workers: usize,
        ends: &mut Vec<[usize; 2]>,
        vertices: &mut Vec<usize>,
    ) {
        ends.clear();
        if workers > 1 {
            ends.resize(keys.len(), [0, 0]);
            let chunk = (keys.len() / (workers * 4)).clamp(64, 4096);
            self.install(|| {
                ends.par_chunks_mut(chunk)
                    .zip(keys.par_chunks(chunk))
                    .for_each_init(Vec::new, |vertices, (slots, part)| {
                        for (slot, &key) in slots.iter_mut().zip(part) {
                            self.bt
                                .unrank(edge_from_key(key).index, 1, self.n, vertices);
                            *slot = [vertices[0], vertices[1]];
                        }
                    });
            });
        } else {
            for &key in keys {
                self.bt
                    .unrank(edge_from_key(key).index, 1, self.n, vertices);
                ends.push([vertices[0], vertices[1]]);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn walk_dim0_row_block(
        &self,
        keys: &[u128],
        ends: &[[usize; 2]],
        workers: usize,
        adjacency: &crate::adjacency::Adjacency,
        rows: &crate::adjacency::Rows,
        union_find: &mut UnionFind,
        diagram: &mut Diagram,
        cycles: &mut Vec<(Simplex, [usize; 2])>,
        columns: &mut Vec<Simplex>,
    ) {
        for (&key, vertices) in keys.iter().zip(ends) {
            let edge = edge_from_key(key);
            let roots = (union_find.find(vertices[0]), union_find.find(vertices[1]));
            if roots.0 != roots.1 {
                emit_dim0_pair(edge, diagram);
                union_find.link(roots.0, roots.1);
            } else if self.max_dim > 0 {
                if workers > 1 {
                    cycles.push((edge, *vertices));
                } else if !rows.pairs_edge(adjacency, vertices[0], vertices[1], edge.diameter) {
                    columns.push(edge);
                }
            }
        }
    }

    pub(super) fn filter_row_cycles(
        &self,
        adjacency: &crate::adjacency::Adjacency,
        rows: &crate::adjacency::Rows,
        workers: usize,
        cycles: &[(Simplex, [usize; 2])],
        keep: &mut Vec<u8>,
        columns: &mut Vec<Simplex>,
    ) {
        if cycles.is_empty() {
            return;
        }
        keep.clear();
        keep.resize(cycles.len(), 0);
        let chunk = (cycles.len() / (workers * 4)).clamp(64, 4096);
        self.install(|| {
            keep.par_chunks_mut(chunk)
                .zip(cycles.par_chunks(chunk))
                .for_each(|(slots, part)| {
                    for (slot, (edge, vertices)) in slots.iter_mut().zip(part) {
                        *slot = u8::from(!rows.pairs_edge(
                            adjacency,
                            vertices[0],
                            vertices[1],
                            edge.diameter,
                        ));
                    }
                });
        });
        columns.extend(
            cycles
                .iter()
                .zip(keep)
                .filter_map(|((edge, _), keep)| (*keep != 0).then_some(*edge)),
        );
    }

    /// Drop the cycle edges that a zero-apparent cofacet already pairs. The
    /// rest are the dim-1 columns, in the order the union-find walk found
    /// them.
    ///
    /// Each edge is tested on its own against the frozen distances, so the
    /// tests may run in any order. Workers take chunks of one shared result
    /// slice. A worker writes only the slots of its own chunk, and the
    /// compaction then reads the slice in walk order, so the columns are the
    /// same at every worker count.
    pub(super) fn dim0_columns(&self, cycles: Vec<(Simplex, [usize; 2])>) -> Vec<Simplex> {
        if !self.params.use_apparent_pairs {
            return cycles.into_iter().map(|(e, _)| e).collect();
        }
        let test =
            |(e, uv): (Simplex, [usize; 2]), verts: &mut Vec<usize>, pairs: &mut PairScratch| {
                verts.clear();
                verts.extend(uv);
                self.is_dim0_column(verts, e, pairs)
            };
        let budget = self.workers(Region::Prefilter, cycles.len());
        if budget <= 1 {
            let mut verts = Vec::new();
            let mut pairs = PairScratch::default();
            return cycles
                .into_iter()
                .filter(|&c| test(c, &mut verts, &mut pairs))
                .map(|(e, _)| e)
                .collect();
        }
        // One byte a slot, not `bool`: a worker writes its own slots, and a
        // plain byte keeps the write a store with no read behind it.
        let mut keep = vec![0u8; cycles.len()];
        let chunk = (cycles.len() / (budget * 4)).clamp(64, 4096);
        self.install(|| {
            keep.par_chunks_mut(chunk)
                .zip(cycles.par_chunks(chunk))
                .for_each_init(
                    || (Vec::new(), PairScratch::default()),
                    |(verts, pairs), (slots, part)| {
                        for (slot, &e) in slots.iter_mut().zip(part) {
                            *slot = u8::from(test(e, verts, pairs));
                        }
                    },
                );
        });
        cycles
            .into_iter()
            .zip(keep)
            .filter_map(|((e, _), keep)| (keep != 0).then_some(e))
            .collect()
    }

    /// True when edge `e` starts a column of dimension 1: no zero-apparent
    /// cofacet pairs it already. `verts` holds the vertices of `e`, and
    /// `pairs` is the caller's scratch.
    pub(super) fn is_dim0_column(
        &self,
        verts: &[usize],
        e: Simplex,
        pairs: &mut PairScratch,
    ) -> bool {
        !self.params.use_apparent_pairs
            || self
                .zero_apparent(verts, e, 1, Pairing::Cofacet, pairs)
                .is_none()
    }
}

/// Sort key for one dim-0 edge: the diameter's bits above the complemented
/// index. A dim-0 edge's diameter is finite and not negative, so its bits
/// order as a `u64` exactly as `f64::total_cmp` orders the value, and the
/// complement turns ascending index into descending. Sorting the keys then
/// costs one `u128` comparison where the two-field comparator cost a
/// normalized float compare and an integer compare.
#[inline]
pub(super) fn edge_key(e: Simplex) -> u128 {
    debug_assert!(e.diameter.is_finite() && e.diameter.is_sign_positive());
    ((e.diameter.to_bits() as u128) << 64) | (!e.index) as u128
}

/// The edge a key came from. Inverse of [`edge_key`].
#[inline]
pub(super) fn edge_from_key(key: u128) -> Simplex {
    Simplex {
        diameter: f64::from_bits((key >> 64) as u64),
        index: !(key as u64),
    }
}

fn row_block_end(sorted: &[u128], start: usize, block: usize) -> usize {
    let mut end = (start + block).min(sorted.len());
    while end < sorted.len() && sorted[end] >> 64 == sorted[end - 1] >> 64 {
        end += 1;
    }
    end
}

fn emit_dim0_pair(edge: Simplex, diagram: &mut Diagram) {
    if edge.diameter > 0.0 {
        diagram.bars.push(Bar {
            dim: 0,
            birth: 0.0,
            death: edge.diameter,
        });
    }
}
