use std::ops::ControlFlow;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::super::cofacet::{Cofacet, Distances, counters};
use super::super::matrix::DistanceMatrix;
use super::super::sparse::SparseDistanceMatrix;

pub(crate) fn positive_predecessor(value: f64) -> f64 {
    assert!(value.is_finite() && value > 0.0);
    f64::from_bits(value.to_bits() - 1)
}
// One cofacet as the bits the frozen rules name: index, sign position,
// and the diameter compared bit for bit rather than by f64 equality.
pub(crate) type Bits = (u64, usize, u64);

pub(crate) fn bits(cf: &Cofacet) -> Bits {
    (cf.index, cf.k, cf.diameter.to_bits())
}

// Collect the full sequence a distance source yields for a base simplex.
pub(crate) fn cofacets<D: Distances>(
    d: &D,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
    upper_only: bool,
) -> Vec<Bits> {
    let mut out = Vec::new();
    d.for_each_cofacet(bt, simplex, verts, dim, upper_only, |cf| {
        out.push(bits(&cf));
        ControlFlow::<()>::Continue(())
    });
    out
}

// The same sequence from the bounded entry point.
pub(crate) fn bounded_cofacets<D: Distances>(
    d: &D,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
    upper_only: bool,
    bound: f64,
) -> Vec<Bits> {
    let mut out = Vec::new();
    d.for_each_cofacet_bounded(bt, simplex, verts, dim, upper_only, bound, |cf| {
        out.push(bits(&cf));
        ControlFlow::<()>::Continue(())
    });
    out
}

pub(crate) fn reference_cofacets(
    sparse: &SparseDistanceMatrix,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
    upper_only: bool,
) -> Vec<Bits> {
    let mut out = Vec::new();
    sparse.for_each_cofacet_reference(bt, simplex, verts, dim, upper_only, |cf| {
        out.push(bits(&cf));
        ControlFlow::<()>::Continue(())
    });
    out
}

pub(crate) fn rank(bt: &BinomialTable, verts: &[usize]) -> u64 {
    verts
        .iter()
        .enumerate()
        .map(|(i, &v)| bt.get(v, i + 1))
        .sum()
}

// The dense matrix that matches a sparse graph: +inf at every absent pair.
pub(crate) fn densify(sparse: &SparseDistanceMatrix) -> DistanceMatrix {
    let n = sparse.len();
    let mut condensed = Vec::new();
    for i in 1..n {
        for j in 0..i {
            condensed.push(SparseDistanceMatrix::get(sparse, i, j));
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

pub(crate) fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn go(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for v in start..n {
            cur.push(v);
            go(v + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    go(0, n, k, &mut Vec::new(), &mut out);
    out
}

// Every simplex of the graph up to `max_dim`, as (simplex, vertices,
// dimension). A vertex set is a simplex when every pair is present.
pub(crate) fn simplices(
    dense: &DistanceMatrix,
    bt: &BinomialTable,
    max_dim: usize,
) -> Vec<(Simplex, Vec<usize>, usize)> {
    let mut out = Vec::new();
    for dim in 0..=max_dim {
        for verts in combinations(dense.len(), dim + 1) {
            let mut diameter = 0.0f64;
            let mut real = true;
            for a in 0..verts.len() {
                for b in 0..a {
                    let d = dense.get(verts[a], verts[b]);
                    if !d.is_finite() {
                        real = false;
                    }
                    diameter = diameter.max(d);
                }
            }
            if !real {
                continue;
            }
            let simplex = Simplex {
                diameter,
                index: rank(bt, &verts),
            };
            out.push((simplex, verts, dim));
        }
    }
    out
}

// The three-way gate on one base simplex. The shipped enumerator, the
// frozen reference, and the dense default with its infinite diameters
// removed must give the same bits in the same order, and the order must
// strictly descend.
pub(crate) fn check_bits(
    label: &str,
    sparse: &SparseDistanceMatrix,
    dense: &DistanceMatrix,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
) {
    for upper_only in [false, true] {
        let expected: Vec<Bits> = cofacets(dense, bt, simplex, verts, dim, upper_only)
            .into_iter()
            .filter(|&(_, _, diameter)| f64::from_bits(diameter).is_finite())
            .collect();
        let reference = reference_cofacets(sparse, bt, simplex, verts, dim, upper_only);
        assert_eq!(
            reference, expected,
            "{label}: reference against dense, verts {verts:?}, upper_only {upper_only}"
        );
        for w in expected.windows(2) {
            assert!(
                w[0].0 > w[1].0,
                "{label}: cofacet indices must strictly descend, verts {verts:?}"
            );
        }
        // The shipped path, reached through the trait the engine calls.
        let shipped = cofacets(sparse, bt, simplex, verts, dim, upper_only);
        assert_eq!(
            shipped, expected,
            "{label}: shipped against dense, verts {verts:?}, upper_only {upper_only}"
        );
    }
}

// Every bound worth testing on one base simplex: the simplex diameter,
// which is the bound the engine passes, each cofacet diameter and a
// value just below it, and the two bounds that keep everything.
pub(crate) fn bounds(simplex: Simplex, full: &[Bits]) -> Vec<f64> {
    let mut out = vec![simplex.diameter, f64::MAX, f64::INFINITY];
    for &(_, _, diameter) in full {
        let d = f64::from_bits(diameter);
        if d.is_finite() && d > simplex.diameter {
            out.push(d);
            out.push(positive_predecessor(d));
        }
    }
    out.sort_unstable_by(f64::total_cmp);
    out.dedup();
    out
}

// The bounded entry point on one base simplex. It must give the
// unbounded sequence filtered to the bound, bits and order alike, on
// the dense source and on the sparse one.
pub(crate) fn check_bounded(
    label: &str,
    sparse: &SparseDistanceMatrix,
    dense: &DistanceMatrix,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
) {
    for upper_only in [false, true] {
        let dense_full = cofacets(dense, bt, simplex, verts, dim, upper_only);
        let sparse_full = cofacets(sparse, bt, simplex, verts, dim, upper_only);
        for bound in bounds(simplex, &dense_full) {
            let under = |full: &[Bits]| -> Vec<Bits> {
                full.iter()
                    .copied()
                    .filter(|&(_, _, diameter)| f64::from_bits(diameter) <= bound)
                    .collect()
            };
            let got = bounded_cofacets(dense, bt, simplex, verts, dim, upper_only, bound);
            assert_eq!(
                got,
                under(&dense_full),
                "{label}: bounded dense at {bound}, verts {verts:?}, upper_only {upper_only}"
            );
            let got = bounded_cofacets(sparse, bt, simplex, verts, dim, upper_only, bound);
            assert_eq!(
                got,
                under(&sparse_full),
                "{label}: bounded sparse at {bound}, verts {verts:?}, upper_only {upper_only}"
            );
        }
    }
}

// The Break gate on one base simplex: the value passes through, the
// callbacks stop at the break, and the enumerator reads no neighbor
// list entry after it. `bound` picks the bounded entry point.
pub(crate) fn check_breaks(
    label: &str,
    sparse: &SparseDistanceMatrix,
    bt: &BinomialTable,
    simplex: Simplex,
    verts: &[usize],
    dim: usize,
    bound: Option<f64>,
) {
    let enumerate = |f: &mut dyn FnMut(Cofacet) -> ControlFlow<u64>, upper_only: bool| match bound {
        Some(bound) => {
            sparse.for_each_cofacet_bounded(bt, simplex, verts, dim, upper_only, bound, f)
        }
        None => sparse.for_each_cofacet(bt, simplex, verts, dim, upper_only, f),
    };
    for upper_only in [false, true] {
        let full = match bound {
            Some(bound) => bounded_cofacets(sparse, bt, simplex, verts, dim, upper_only, bound),
            None => cofacets(sparse, bt, simplex, verts, dim, upper_only),
        };

        // A run that never breaks: no Break value, and one mark of the
        // candidate counter per callback.
        counters::reset();
        let mut marks = Vec::new();
        let out = enumerate(
            &mut |_| {
                marks.push(counters::read().candidates);
                ControlFlow::<u64>::Continue(())
            },
            upper_only,
        );
        let events = counters::read();
        assert_eq!(out, None, "{label}: a run without a Break");
        assert_eq!(events.callbacks as usize, full.len(), "{label}: callbacks");
        assert_eq!(events.breaks, 0, "{label}: breaks without a Break");

        for m in 0..full.len() {
            let sentinel = 0xbeef_0000_u64 + m as u64;
            counters::reset();
            let mut seen = Vec::new();
            let mut done = false;
            let out = enumerate(
                &mut |cf| {
                    assert!(!done, "{label}: called back after a Break at {m}");
                    seen.push(bits(&cf));
                    if seen.len() == m + 1 {
                        done = true;
                        ControlFlow::Break(sentinel)
                    } else {
                        ControlFlow::Continue(())
                    }
                },
                upper_only,
            );
            let events = counters::read();
            assert_eq!(out, Some(sentinel), "{label}: Break value at {m}");
            assert_eq!(seen, full[..=m], "{label}: callback prefix at {m}");
            assert_eq!(
                events.callbacks as usize,
                m + 1,
                "{label}: callbacks at {m}"
            );
            assert_eq!(events.breaks, 1, "{label}: breaks at {m}");
            assert_eq!(
                events.candidates, marks[m],
                "{label}: neighbor list entries read after the Break at {m}"
            );
        }
    }
}

// Both gates on every simplex of a graph up to `max_dim`.
pub(crate) fn check_graph(label: &str, sparse: &SparseDistanceMatrix, max_dim: usize) {
    let dense = densify(sparse);
    let n = sparse.len();
    let bt = BinomialTable::new(n.max(1), max_dim + 2).unwrap();
    for (simplex, verts, dim) in simplices(&dense, &bt, max_dim) {
        check_bits(label, sparse, &dense, &bt, simplex, &verts, dim);
        check_bounded(label, sparse, &dense, &bt, simplex, &verts, dim);
        check_breaks(label, sparse, &bt, simplex, &verts, dim, None);
        check_breaks(
            label,
            sparse,
            &bt,
            simplex,
            &verts,
            dim,
            Some(simplex.diameter),
        );
    }
}

pub(crate) fn graph(n: usize, triplets: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(n, triplets).unwrap()
}
