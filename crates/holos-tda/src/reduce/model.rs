use std::collections::BinaryHeap;

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::combinadic::BinomialTable;
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;
use crate::union_find::UnionFind;
use crate::{Bar, RipsParams};

/// The pivot registry for one dimension: pivot coefficient and column
/// position, keyed by pivot index. Consumed as the clearing set of the next
/// dimension.
pub(crate) type Pivots = FxHashMap<u64, (u64, usize)>;

/// One term of an H1 cochain before its edge index is decoded.
#[derive(Debug, Clone)]
pub(crate) struct RawH1Term {
    pub(crate) simplex: Simplex,
    pub(crate) coefficient: u64,
}

/// One interval and the reduction column that represents it.
#[derive(Debug, Clone)]
pub(crate) struct RawH1Class {
    pub(crate) bar: Bar,
    pub(crate) scale: f64,
    pub(crate) birth: Simplex,
    pub(crate) death: Option<Simplex>,
    pub(crate) terms: Vec<RawH1Term>,
}

pub(super) struct Dim0Walk {
    pub(super) union_find: UnionFind,
    pub(super) cycles: Vec<(Simplex, [usize; 2])>,
    pub(super) columns: Vec<Simplex>,
}

pub(super) struct SerialReduction<'a> {
    pub(super) pivots: Pivots,
    pub(super) entries: Vec<Entry>,
    pub(super) offsets: Vec<usize>,
    pub(super) coboundary: BinaryHeap<HeapEntry>,
    pub(super) reduction: BinaryHeap<HeapEntry>,
    pub(super) cofacets: Vec<Entry>,
    pub(super) vertices: Vec<usize>,
    pub(super) cofacet_vertices: Vec<usize>,
    pub(super) pairs: PairScratch,
    pub(super) essential_terms: Vec<Entry>,
    pub(super) classes: Option<&'a mut Vec<RawH1Class>>,
}

impl<'a> SerialReduction<'a> {
    pub(super) fn new(column_count: usize, classes: Option<&'a mut Vec<RawH1Class>>) -> Self {
        Self {
            pivots: FxHashMap::with_capacity_and_hasher(column_count, FxBuildHasher),
            entries: Vec::new(),
            offsets: vec![0],
            coboundary: BinaryHeap::new(),
            reduction: BinaryHeap::new(),
            cofacets: Vec::new(),
            vertices: Vec::new(),
            cofacet_vertices: Vec::new(),
            pairs: PairScratch::default(),
            essential_terms: Vec::new(),
            classes,
        }
    }
}

/// Vertex and distance buffers for the apparent-pair kernels. One per
/// worker: the kernels write here and nowhere else, so two workers never
/// share a buffer.
#[derive(Default)]
pub(crate) struct PairScratch {
    /// Vertices of the facet under test.
    pub(super) facet: Vec<usize>,
    /// Vertices of the cofacet under test.
    pub(super) cofacet: Vec<usize>,
    /// Pairwise distances of the queried simplex.
    pub(super) base: PairTable,
    /// Pairwise distances of the cofacet under test.
    pub(super) cofacet_table: PairTable,
    /// Distance from the added vertex to each queried simplex vertex.
    pub(super) added: Vec<f64>,
}

/// Pairwise distances of one simplex, keyed by vertex position: the distance
/// between positions p and q with p < q sits at q * (q - 1) / 2 + p.
///
/// A table fills one vertex position at a time, so a caller that stops at an
/// early facet reads only the distances that facet needed. `owner` names the
/// simplex the entries belong to, so a second query on the same simplex
/// reads nothing.
#[derive(Default)]
pub(crate) struct PairTable {
    pub(super) d: Vec<f64>,
    /// Vertex positions whose distances are present. Every pair below this
    /// position is in `d`.
    pub(super) filled: usize,
    /// The simplex the entries describe, as (index, vertex count).
    pub(super) owner: Option<(u64, usize)>,
}

impl PairTable {
    pub(super) fn holds(&self, owner: (u64, usize)) -> bool {
        self.owner == Some(owner)
    }

    pub(super) fn reset(&mut self, owner: (u64, usize)) {
        self.d.clear();
        self.filled = 0;
        self.owner = Some(owner);
    }

    /// Distance between positions `p` and `q`, with `p` below `q`.
    #[inline]
    pub(super) fn at(&self, p: usize, q: usize) -> f64 {
        self.d[q * (q - 1) / 2 + p]
    }

    /// Record an edge's one distance, which is its diameter.
    pub(super) fn set_edge(&mut self, diameter: f64) {
        self.d.push(diameter);
        self.filled = 2;
    }

    /// Read the distances of the vertex positions below `upto` that are
    /// still missing. Each read takes the lower vertex first.
    pub(super) fn fill<D: Distances>(&mut self, dist: &D, vertices: &[usize], upto: usize) {
        while self.filled < upto {
            let q = self.filled;
            let vq = vertices[q];
            for &vp in &vertices[..q] {
                self.d.push(dist.get(vp, vq));
            }
            self.filled += 1;
        }
    }

    /// Fill from a base simplex's table and the distances from one added
    /// vertex to each base vertex. `at` is the added vertex's position in
    /// the cofacet. Reads no distance.
    pub(super) fn fill_cofacet(
        &mut self,
        owner: (u64, usize),
        base: &PairTable,
        added: &[f64],
        at: usize,
    ) {
        let m = added.len() + 1;
        self.reset(owner);
        for q in 0..m {
            for p in 0..q {
                let x = if q == at {
                    added[p]
                } else if p == at {
                    added[q - 1]
                } else {
                    let base_p = if p < at { p } else { p - 1 };
                    let base_q = if q < at { q } else { q - 1 };
                    base.at(base_p, base_q)
                };
                self.d.push(x);
            }
        }
        self.filled = m;
    }

    /// Largest distance between two vertex positions below `m`, skipping
    /// position `omit`. A simplex with fewer than two vertices has diameter
    /// 0, so the empty maximum is 0.
    pub(super) fn omit_max(&self, m: usize, omit: usize) -> f64 {
        let mut d = 0.0f64;
        for q in 0..m {
            if q == omit {
                continue;
            }
            for p in 0..q {
                if p != omit {
                    d = d.max(self.at(p, q));
                }
            }
        }
        d
    }
}

/// Which partner of a zero-apparent pair the caller asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pairing {
    /// The facet whose zero-apparent cofacet is the queried simplex.
    Facet,
    /// The cofacet whose zero-apparent facet is the queried simplex.
    Cofacet,
}

/// A confirmed zero-apparent pair, seen from the queried simplex. The
/// classifier returns it only after both halves of the test agree, so a
/// caller that holds one never repeats the test. The partner's vertex set
/// stays in the scratch the call was given.
#[derive(Clone, Copy)]
pub(crate) struct ApparentPair {
    /// The partner: the facet under [`Pairing::Facet`], the cofacet under
    /// [`Pairing::Cofacet`].
    pub(crate) other: Simplex,
    /// Position of the differing vertex in the larger of the two simplices.
    /// Under [`Pairing::Facet`] it is the boundary sign exponent.
    pub(crate) k: usize,
}

pub(crate) struct Engine<'a, C: Coeffs, D: Distances> {
    pub(crate) dist: &'a D,
    pub(crate) bt: BinomialTable,
    pub(crate) n: usize,
    /// The filtration threshold, with `+inf` replaced by `f64::MAX`. A
    /// diameter is in the complex exactly when it is at or below this value:
    /// `+inf` fails against `f64::MAX`, and no finite diameter exceeds it.
    pub(super) effective_threshold: f64,
    pub(crate) max_dim: usize,
    pub(crate) params: &'a RipsParams,
    pub(crate) ops: C,
    /// One worker pool for the whole run. Every parallel region installs onto
    /// it, so the pool is built once and the thread count is honored exactly.
    /// `None` runs serially.
    pub(super) pool: Option<rayon::ThreadPool>,
    /// Adjacency bitsets for the dim-0 apparent test, when the rows fit
    /// the budget.
    pub(super) adjacency: Option<crate::adjacency::Adjacency>,
}
