use super::cofacet_fixtures::*;
use super::cofacet_support::*;
use super::common::Rng;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::super::cofacet::{INLINE_VERTS, counters};
use super::super::matrix::DistanceMatrix;
use super::super::sparse::SparseDistanceMatrix;

#[test]
fn sparse_cofacets_match_dense_default() {
    let mut rng = Rng::new(0xc0fa_ce75_0000_0001);
    let mut trials = 0usize;
    for _ in 0..4000 {
        let n = 4 + rng.below(9);
        let (sparse, dense) = random_graph(&mut rng, n);
        let bt = BinomialTable::new(n, 6).unwrap();
        let dim = 1 + rng.below(3); // base simplex dimension 1..=3
        if dim + 1 > n {
            continue;
        }
        // A genuine base simplex: distinct vertices, all pairs present.
        let mut verts: Vec<usize> = Vec::new();
        while verts.len() < dim + 1 {
            let v = rng.below(n);
            if !verts.contains(&v) {
                verts.push(v);
            }
        }
        verts.sort_unstable();
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
            index: rank(&bt, &verts),
        };
        check_bits("random", &sparse, &dense, &bt, simplex, &verts, dim);
        check_bounded("random", &sparse, &dense, &bt, simplex, &verts, dim);
        trials += 1;
    }
    assert!(
        trials > 500,
        "too few genuine simplices exercised: {trials}"
    );
}

// Degenerate intersections stay in lockstep with the dense default: an
// empty pivot neighbor list (isolated vertex), an empty intersection
// with both endpoints non-empty, and an ordinary non-empty case.
#[test]
fn sparse_cofacets_empty_intersections() {
    // Triangle {0,1,2}, a disjoint edge 3-4, and an isolated vertex 5.
    let sparse = SparseDistanceMatrix::from_triplets(
        6,
        &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0), (3, 4, 2.0)],
    )
    .unwrap();
    let inf = f64::INFINITY;
    let dense = DistanceMatrix::from_condensed(vec![
        1.0, // 1-0
        1.0, 1.0, // 2-0, 2-1
        inf, inf, inf, // 3-*
        inf, inf, inf, 2.0, // 4-*, 4-3
        inf, inf, inf, inf, inf, // 5-*
    ])
    .unwrap();
    let bt = BinomialTable::new(6, 6).unwrap();

    // {0,1}: common neighbor 2 (non-empty). {0,3}: 0->{1,2}, 3->{4}, no
    // common vertex (empty intersection, both lists non-empty). {0,5}:
    // vertex 5 is isolated, so the pivot list is empty.
    for verts in [[0usize, 1usize], [0, 3], [0, 5]] {
        let d01 = dense.get(verts[0], verts[1]);
        let simplex = Simplex {
            diameter: d01,
            index: rank(&bt, &verts),
        };
        check_bits("degenerate", &sparse, &dense, &bt, simplex, &verts, 1);
    }
}

// The named adversarial graphs, every simplex of each, by bits and by
// Break position.
#[test]
fn adversarial_graphs_match_the_reference() {
    for (label, sparse) in adversarial_fixtures() {
        check_graph(label, &sparse, 3.min(sparse.len().saturating_sub(1)));
    }
}

// Randomized shapes the fixed graphs do not reach: a complete graph, a
// graph so thin that most intersections are empty, an all-equal graph,
// and one with duplicate points. Every simplex of every draw is checked.
#[test]
fn random_shapes_match_the_reference() {
    let mut rng = Rng::new(0x5ea5_0f17_0000_0003);
    let shapes: [(&str, usize, &[f64]); 4] = [
        ("complete", 1000, &[1.0, 2.0, 3.0]),
        ("thin", 120, &[1.0, 2.0]),
        ("all equal", 700, &[2.0]),
        ("duplicate points", 700, &[0.0, 0.0, 1.0]),
    ];
    for (label, present, palette) in shapes {
        for _ in 0..12 {
            let n = 5 + rng.below(4);
            let sparse = random_graph_shaped(&mut rng, n, present, palette);
            check_graph(label, &sparse, 3.min(n - 1));
        }
    }
}

// A simplex wider than the inline cursor array must enumerate from the
// heap and stay bit-exact. Nothing narrower may allocate.
#[test]
fn wide_simplices_spill_to_the_heap() {
    let n = 20;
    let mut triplets = Vec::new();
    for a in 0..n {
        for b in 0..a {
            triplets.push((a, b, 1.0 + ((a * 3 + b) % 5) as f64));
        }
    }
    let sparse = graph(n, &triplets);
    let dense = densify(&sparse);
    let bt = BinomialTable::new(n, INLINE_VERTS + 4).unwrap();
    // Widths on both sides of the inline bound, including the first
    // width that spills.
    for width in [
        INLINE_VERTS - 1,
        INLINE_VERTS,
        INLINE_VERTS + 1,
        INLINE_VERTS + 2,
    ] {
        let verts: Vec<usize> = (0..width).collect();
        let dim = width - 1;
        let mut diameter = 0.0f64;
        for a in 0..width {
            for b in 0..a {
                diameter = diameter.max(dense.get(verts[a], verts[b]));
            }
        }
        let simplex = Simplex {
            diameter,
            index: rank(&bt, &verts),
        };
        check_bits("wide", &sparse, &dense, &bt, simplex, &verts, dim);
        check_bounded("wide", &sparse, &dense, &bt, simplex, &verts, dim);
        check_breaks("wide", &sparse, &bt, simplex, &verts, dim, None);

        counters::reset();
        let got = cofacets(&sparse, &bt, simplex, &verts, dim, false);
        let spills = counters::read().spills;
        assert!(!got.is_empty(), "width {width}: nothing to enumerate");
        if width > INLINE_VERTS {
            assert_eq!(spills, 1, "width {width}: the cursors must spill once");
        } else {
            assert_eq!(spills, 0, "width {width}: the cursors must not allocate");
        }
    }
}

// A dense source that counts the distance reads its enumerator makes.
