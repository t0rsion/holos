use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;

use super::dim0::{edge_from_key, edge_key};
use super::*;
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, Fp, HeapEntry, Z2, counters};
use crate::simplex::Simplex;
use crate::union_find::UnionFind;
use crate::{Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix};

#[path = "tests/counters.rs"]
mod heap_counters;

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn cloud(rng: &mut Rng, n: usize, coord_dim: usize) -> DistanceMatrix {
    let points: Vec<Vec<f64>> = (0..n)
        .map(|_| (0..coord_dim).map(|_| rng.unit()).collect())
        .collect();
    DistanceMatrix::from_points(&points).unwrap()
}

/// A lattice, where distances repeat and the heap sees many ties.
fn lattice(side: usize) -> DistanceMatrix {
    let mut points = Vec::new();
    for i in 0..side {
        for j in 0..side {
            points.push(vec![i as f64, j as f64]);
        }
    }
    DistanceMatrix::from_points(&points).unwrap()
}

/// One heap entry as the bits that must not move.
type Bits = (u64, u64);

fn bits(e: Entry) -> Bits {
    (e.diameter.to_bits(), e.payload)
}

/// What one construction of a working column did.
#[derive(Default)]
struct Built {
    pivot: Option<Bits>,
    popped: Vec<Bits>,
    entries: usize,
    capacity: usize,
    comparisons: u64,
}

/// Totals over a whole run, for the mechanism report.
#[derive(Default)]
struct Totals {
    entries: usize,
    capacity: usize,
    comparisons: u64,
}

impl Totals {
    fn add(&mut self, b: &Built) {
        self.entries += b.entries;
        self.capacity = self.capacity.max(b.capacity);
        self.comparisons += b.comparisons;
    }
}

/// Drain `heap` with the field's cancellation and record what it gave.
fn drain<C: Coeffs, D: Distances>(
    engine: &Engine<'_, C, D>,
    heap: &mut BinaryHeap<HeapEntry>,
    pivot: Option<Entry>,
    built: &mut Built,
) {
    built.pivot = pivot.map(bits);
    built.entries = heap.len();
    built.capacity = heap.capacity();
    while let Some(e) = engine.ops.pop_pivot(heap) {
        built.popped.push(bits(e));
    }
}

/// A graph with a small distance palette, so many triangles tie their
/// edges; `keep` of four pairs get an edge, the rest are absent.
fn tie_graph(rng: &mut Rng, n: usize, keep: u64, zero: bool) -> DistanceMatrix {
    let palette: &[f64] = if zero {
        &[0.0, 0.0, 1.0, 1.0]
    } else {
        &[0.5, 1.0, 1.0, 1.5, 2.0, 2.0, 3.0]
    };
    let mut condensed = Vec::new();
    for _ in 0..n * (n - 1) / 2 {
        condensed.push(if rng.next_u64() % 4 < keep {
            palette[(rng.next_u64() % palette.len() as u64) as usize]
        } else {
            f64::INFINITY
        });
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

/// The row test against the neighbor-list classifier, edge by edge: on
/// every cycle edge of a graph, with the rows activated exactly (as the
/// serial walk does) and over-activated through the whole graph (as the
/// parallel walk may), the two must agree. Covers `w` above, between,
/// and below the edge's ends, zero distances, all-equal graphs,
/// absent edges, and disconnected graphs.
#[test]
fn rows_agree_with_the_classifier_on_every_cycle_edge() {
    let mut rng = Rng::new(0x51ed_270e_c9c0_ffee);
    let mut tested = 0usize;
    let mut params = RipsParams {
        threshold: Some(2.5),
        ..RipsParams::default()
    };
    for case in 0..40 {
        let n = 8 + case % 23;
        let dist = tie_graph(&mut rng, n, 1 + case as u64 % 3, case % 5 == 0);
        let threshold = if case % 7 == 0 { f64::MAX } else { 2.5 };
        params.threshold = Some(threshold);
        let engine = Engine::new(&dist, &params, Z2).unwrap();
        let Some(adjacency) = crate::adjacency::Adjacency::build_gated(&dist, threshold, 0) else {
            continue;
        };
        let mut sorted = engine.edges();
        sorted.sort_unstable_by(|a, b| {
            a.diameter
                .total_cmp(&b.diameter)
                .then(b.index.cmp(&a.index))
        });
        let mut full = crate::adjacency::Rows::new(n);
        let mut verts = Vec::new();
        for e in &sorted {
            engine.bt.unrank(e.index, 1, n, &mut verts);
            full.set(verts[0], verts[1]);
        }
        let mut exact = crate::adjacency::Rows::new(n);
        let mut uf = UnionFind::new(n);
        let mut pairs = PairScratch::default();
        let mut group = 0;
        while group < sorted.len() {
            let d = sorted[group].diameter;
            let mut end = group;
            while end < sorted.len() && sorted[end].diameter == d {
                engine.bt.unrank(sorted[end].index, 1, n, &mut verts);
                exact.set(verts[0], verts[1]);
                end += 1;
            }
            for e in &sorted[group..end] {
                engine.bt.unrank(e.index, 1, n, &mut verts);
                let (ru, rv) = (uf.find(verts[0]), uf.find(verts[1]));
                if ru != rv {
                    uf.link(ru, rv);
                    continue;
                }
                let classifier = engine
                    .zero_apparent(&verts, *e, 1, Pairing::Cofacet, &mut pairs)
                    .is_some();
                let by_exact = exact.pairs_edge(&adjacency, verts[0], verts[1], d);
                let by_full = full.pairs_edge(&adjacency, verts[0], verts[1], d);
                assert_eq!(
                    by_exact, classifier,
                    "case {case} edge {:?} exact rows",
                    verts
                );
                assert_eq!(
                    by_full, classifier,
                    "case {case} edge {:?} full rows",
                    verts
                );
                tested += 1;
            }
            group = end;
        }
    }
    assert!(tested > 2000, "{tested} cycle edges tested");
}

/// Build every column's working coboundary both ways and require the
/// same pivot and the same drained sequence. Returns the reference and
/// the shipped totals.
fn heaps_agree<C: Coeffs + Sync>(
    label: &str,
    dist: &DistanceMatrix,
    params: &RipsParams,
    ops: C,
) -> (Totals, Totals) {
    let engine = Engine::new(dist, params, ops).unwrap();
    let mut diagram = Diagram::default();
    let edges = engine.edges();
    let mut columns = engine.dim0_pairs(&edges, &mut diagram);
    let mut simplices = edges;
    let mut prev_pivots: Pivots = FxHashMap::default();
    let mut reference = Totals::default();
    let mut shipped = Totals::default();

    let mut heap_a: BinaryHeap<HeapEntry> = BinaryHeap::new();
    let mut heap_b: BinaryHeap<HeapEntry> = BinaryHeap::new();
    let mut buf_a: Vec<Entry> = Vec::new();
    let mut buf_b: Vec<Entry> = Vec::new();
    let mut verts = Vec::new();
    let mut cofacet_verts = Vec::new();
    let mut pairs = PairScratch::default();

    for dim in 1..=engine.max_dim {
        for &column in &columns {
            // The pivot map decides whether the emergent shortcut can
            // fire, so both answers are exercised. Under `true` no
            // shortcut fires and every column builds its heap.
            for claimed in [false, true] {
                let mut a = Built::default();
                let mut b = Built::default();
                heap_a.clear();
                counters::reset();
                let pivot = engine.init_coboundary_reference(
                    column,
                    dim,
                    |_| claimed,
                    &mut heap_a,
                    &mut buf_a,
                    &mut verts,
                    &mut cofacet_verts,
                    &mut pairs,
                );
                a.comparisons = counters::comparisons();
                drain(&engine, &mut heap_a, pivot, &mut a);

                heap_b.clear();
                counters::reset();
                let pivot = engine.init_coboundary(
                    column,
                    dim,
                    |_| claimed,
                    &mut heap_b,
                    &mut buf_b,
                    &mut verts,
                    &mut cofacet_verts,
                    &mut pairs,
                );
                b.comparisons = counters::comparisons();
                drain(&engine, &mut heap_b, pivot, &mut b);

                assert_eq!(a.pivot, b.pivot, "{label}: init pivot, dim {dim}");
                assert_eq!(a.popped, b.popped, "{label}: init pops, dim {dim}");
                assert_eq!(a.entries, b.entries, "{label}: init entries, dim {dim}");
                reference.add(&a);
                shipped.add(&b);

                let mut a = Built::default();
                let mut b = Built::default();
                heap_a.clear();
                counters::reset();
                engine.build_full_coboundary_reference(column, dim, &mut heap_a, &mut verts);
                a.comparisons = counters::comparisons();
                drain(&engine, &mut heap_a, None, &mut a);
                heap_b.clear();
                counters::reset();
                engine.build_full_coboundary(column, dim, &mut heap_b, &mut verts);
                b.comparisons = counters::comparisons();
                drain(&engine, &mut heap_b, None, &mut b);
                assert_eq!(a.popped, b.popped, "{label}: full pops, dim {dim}");
                reference.add(&a);
                shipped.add(&b);
            }
        }
        let pivots = engine.reduce_dimension(&columns, dim, &prev_pivots, &mut diagram);
        if dim < engine.max_dim {
            (simplices, columns) =
                engine.assemble(&simplices, dim + 1, &pivots, dim + 1 < engine.max_dim);
        }
        prev_pivots = pivots;
    }
    (reference, shipped)
}

// The bulk heapify must give the pivot and the whole cancelled pop
// sequence the sift-up loop gives, over Z/2 and over odd primes, with
// the emergent shortcut on and off.
#[test]
fn the_bulk_heapify_pops_the_same_entries() {
    let mut rng = Rng::new(0x8eaa_1f70_0000_0001);
    let inputs = [
        ("cube", cloud(&mut rng, 40, 3)),
        ("plane", cloud(&mut rng, 30, 2)),
        ("lattice", lattice(5)),
    ];
    let mut entries = 0usize;
    for (label, dist) in &inputs {
        for max_dim in [1usize, 2] {
            for emergent in [true, false] {
                let mut params = RipsParams::new(max_dim);
                params.use_emergent_pairs = emergent;
                // The engine takes its field directly, so the modulus
                // here picks the arithmetic and not a parameter.
                for modulus in [2u64, 3, 5] {
                    let (a, b) = if modulus == 2 {
                        heaps_agree(label, dist, &params, Z2)
                    } else {
                        heaps_agree(label, dist, &params, Fp::new(modulus))
                    };
                    assert_eq!(a.entries, b.entries, "{label}: heap entries");
                    entries += b.entries;
                }
            }
        }
    }
    assert!(entries > 0, "no working column was built");
}

/// A sparse matrix that keeps the pairs at or below `keep` of a cloud,
/// so the sparse `get` and the sparse enumerator run in the test.
fn sparse_cloud(rng: &mut Rng, n: usize, coord_dim: usize, keep: f64) -> SparseDistanceMatrix {
    let dense = cloud(rng, n, coord_dim);
    let mut triplets = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dense.get(i, j);
            if d <= keep {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

/// One apparent-pair answer as the bits that must not move.
fn pair_bits(p: Option<ApparentPair>) -> Option<(u64, u64, usize)> {
    p.map(|p| (p.other.diameter.to_bits(), p.other.index, p.k))
}

/// Query both facet searches on the same simplex and require the same
/// answer, down to the facet's vertices. Returns the number of facet
/// searches that found a facet.
fn facet_searches_agree<C: Coeffs + Sync, D: Distances + Sync>(
    label: &str,
    engine: &Engine<'_, C, D>,
    simplices: &[Simplex],
    dim: usize,
) -> usize {
    let mut verts = Vec::new();
    let mut shipped_verts = Vec::new();
    let mut reference_verts = Vec::new();
    let mut found = 0usize;
    for &s in simplices {
        engine.bt.unrank(s.index, dim, engine.n, &mut verts);
        // A fresh table each time, so the search fills it from cold.
        let mut table = PairTable::default();
        let shipped = engine.zero_pivot_facet_with(&verts, s, &mut table, &mut shipped_verts);
        let reference = engine.zero_pivot_facet_reference(&verts, s, dim, &mut reference_verts);
        let key = |r: Option<(Simplex, usize, usize)>| {
            r.map(|(f, k, removed)| (f.diameter.to_bits(), f.index, k, removed))
        };
        assert_eq!(
            key(shipped),
            key(reference),
            "{label}: facet search, dim {dim}"
        );
        if shipped.is_some() {
            assert_eq!(
                shipped_verts, reference_verts,
                "{label}: facet vertices, dim {dim}"
            );
            found += 1;
        }
    }
    found
}

/// Classify every simplex both ways and in both directions, and require
/// the same answer. Returns the number of classifications made.
fn classifiers_agree<C: Coeffs + Sync, D: Distances + Sync>(
    label: &str,
    engine: &Engine<'_, C, D>,
    simplices: &[Simplex],
    dim: usize,
) -> usize {
    let mut verts = Vec::new();
    let mut shipped = PairScratch::default();
    let mut reference = PairScratch::default();
    let mut tested = 0usize;
    for &s in simplices {
        engine.bt.unrank(s.index, dim, engine.n, &mut verts);
        for pairing in [Pairing::Cofacet, Pairing::Facet] {
            if pairing == Pairing::Facet && dim == 0 {
                continue;
            }
            let a = engine.zero_apparent(&verts, s, dim, pairing, &mut shipped);
            let b = engine.zero_apparent_reference(&verts, s, dim, pairing, &mut reference);
            assert_eq!(pair_bits(a), pair_bits(b), "{label}: classifier, dim {dim}");
            tested += 1;
        }
    }
    tested
}

/// Walk the complex of `dist` and check both kernels at every dimension.
fn kernels_agree<D: Distances + Sync>(label: &str, dist: &D, max_dim: usize) -> (usize, usize) {
    let params = RipsParams::new(max_dim);
    let engine = Engine::new(dist, &params, Z2).unwrap();
    let mut diagram = Diagram::default();
    let edges = engine.edges();
    let mut columns = engine.dim0_pairs(&edges, &mut diagram);
    let mut simplices = edges;
    let (mut found, mut tested) = (0usize, 0usize);
    for dim in 1..=engine.max_dim {
        found += facet_searches_agree(label, &engine, &simplices, dim);
        tested += classifiers_agree(label, &engine, &simplices, dim);
        let pivots = engine.reduce_dimension(&columns, dim, &Pivots::default(), &mut diagram);
        if dim < engine.max_dim {
            (simplices, columns) =
                engine.assemble(&simplices, dim + 1, &pivots, dim + 1 < engine.max_dim);
        }
    }
    (found, tested)
}

// The table-based facet diameters and the arithmetic facet index must
// give what the search-and-read path gives, bit for bit, on dense and
// sparse sources and on input with many tied distances.
#[test]
fn the_facet_kernels_agree_with_the_reference() {
    let mut rng = Rng::new(0x8eaa_1f70_0000_0003);
    let (mut found, mut tested) = (0usize, 0usize);
    for (label, dist, max_dim) in [
        ("cube", cloud(&mut rng, 36, 3), 3),
        ("plane", cloud(&mut rng, 30, 2), 2),
        ("lattice", lattice(5), 2),
    ] {
        let (f, t) = kernels_agree(label, &dist, max_dim);
        found += f;
        tested += t;
    }
    for (label, keep, max_dim) in [("sparse-near", 0.35f64, 3), ("sparse-wide", 0.8, 2)] {
        let dist = sparse_cloud(&mut rng, 40, 3, keep);
        let (f, t) = kernels_agree(label, &dist, max_dim);
        found += f;
        tested += t;
    }
    assert!(found > 0, "no facet search found a facet");
    assert!(tested > 0, "no simplex was classified");
}

// The key sort must put the dim-0 edges where the two-field comparator
// put them, on dense and sparse sources.
#[test]
fn the_edge_key_sort_matches_the_comparator() {
    let mut rng = Rng::new(0x8eaa_1f70_0000_0004);
    let order = |a: &Simplex, b: &Simplex| {
        a.diameter
            .total_cmp(&b.diameter)
            .then(b.index.cmp(&a.index))
    };
    let dense = [cloud(&mut rng, 120, 3), lattice(9)];
    let sparse = [sparse_cloud(&mut rng, 150, 3, 0.4)];
    let mut edge_sets: Vec<Vec<Simplex>> = Vec::new();
    let params = RipsParams::new(1);
    for dist in &dense {
        edge_sets.push(Engine::new(dist, &params, Z2).unwrap().edges());
    }
    for dist in &sparse {
        edge_sets.push(Engine::new(dist, &params, Z2).unwrap().edges());
    }
    for edges in &edge_sets {
        assert!(!edges.is_empty(), "no edge to sort");
        let mut want = edges.clone();
        want.sort_unstable_by(order);
        let mut keys: Vec<u128> = edges.iter().map(|&e| edge_key(e)).collect();
        keys.sort_unstable();
        let got: Vec<Simplex> = keys.into_iter().map(edge_from_key).collect();
        assert_eq!(want.len(), got.len());
        for (w, g) in want.iter().zip(&got) {
            assert_eq!(
                (w.diameter.to_bits(), w.index),
                (g.diameter.to_bits(), g.index),
                "key sort order"
            );
        }
    }
}
