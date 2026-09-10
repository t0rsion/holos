use super::cofacet_fixtures::random_graph;
use super::cofacet_support::*;
use super::common::Rng;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::super::cofacet::{Distances, counters};
use super::super::matrix::DistanceMatrix;
use super::super::sparse::SparseDistanceMatrix;

struct Counting<'a> {
    inner: &'a DistanceMatrix,
    reads: std::cell::Cell<usize>,
}

impl Distances for Counting<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        self.reads.set(self.reads.get() + 1);
        self.inner.get(i, j)
    }
    fn default_threshold(&self) -> f64 {
        self.inner.enclosing_radius()
    }
    fn for_each_edge(&self, f: impl FnMut(usize, usize, f64)) {
        Distances::for_each_edge(self.inner, f)
    }
}

// The bounded fold reads distances until one exceeds the bound, and it
// stops there. The edge {0,1} of this four-point matrix has the cofacets
// 3 and then 2. Vertex 3 is far from vertex 0, so it costs one read and
// no callback; vertex 2 is near both, so it costs two reads and reaches
// the callback. The unbounded walk reads all four distances and reports
// both cofacets.
#[test]
fn the_bounded_fold_stops_at_the_first_distance_above_the_bound() {
    // Condensed order: (1,0), (2,0), (2,1), (3,0), (3,1), (3,2).
    let dense = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 5.0, 1.0, 1.0]).unwrap();
    let counting = Counting {
        inner: &dense,
        reads: std::cell::Cell::new(0),
    };
    let bt = BinomialTable::new(4, 3).unwrap();
    let verts = [0usize, 1usize];
    let simplex = Simplex {
        diameter: 1.0,
        index: rank(&bt, &verts),
    };

    let full = cofacets(&counting, &bt, simplex, &verts, 1, false);
    assert_eq!(counting.reads.replace(0), 4, "reads of the unbounded walk");
    assert_eq!(full.len(), 2, "cofacets of the unbounded walk");

    let got = bounded_cofacets(&counting, &bt, simplex, &verts, 1, false, simplex.diameter);
    assert_eq!(counting.reads.replace(0), 3, "reads of the bounded walk");
    let expected: Vec<Bits> = full
        .into_iter()
        .filter(|&(_, _, diameter)| f64::from_bits(diameter) <= simplex.diameter)
        .collect();
    assert_eq!(got, expected, "cofacets of the bounded walk");
}

// Under `upper_only` the merge stops at the first candidate that is not
// above every simplex vertex. The fixture puts two qualifying candidates
// above the edge {2, 3} and two failing ones below it, so a walk that
// read past the boundary would confirm 1 and 0 against the second list
// and count more entries.
#[test]
fn the_upper_only_merge_stops_at_the_top_simplex_vertex() {
    let far = 5.0;
    let near = 1.0;
    let mut triplets = vec![(2usize, 3usize, near)];
    for v in [0usize, 1] {
        triplets.push((2, v, far));
        triplets.push((3, v, far));
    }
    for v in [4usize, 5] {
        triplets.push((2, v, near));
        triplets.push((3, v, near));
    }
    let sparse = SparseDistanceMatrix::from_triplets(6, &triplets).unwrap();
    let bt = BinomialTable::new(6, 3).unwrap();
    let verts = [2usize, 3usize];
    let simplex = Simplex {
        diameter: near,
        index: rank(&bt, &verts),
    };

    counters::reset();
    let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, true, simplex.diameter);
    let events = counters::read();
    assert_eq!(got.len(), 2, "cofacets above the edge");
    // The two entries of vertex 2's list above the edge, and one entry of
    // vertex 3's list per confirmed candidate. Nothing below the edge is
    // read at all.
    assert_eq!(events.candidates, 4, "neighbor list entries read");
}

// A bound no stored distance reaches drops nothing, so the walk reads
// exactly what the unbounded walk reads and reports the same cofacets.
#[test]
fn a_vacuous_bound_drops_nothing() {
    let mut rng = Rng::new(0x51ed_2701);
    let (sparse, _) = random_graph(&mut rng, 7);
    let bt = BinomialTable::new(7, 3).unwrap();
    let (u, v) = sparse
        .edges()
        .map(|(u, v, _)| (u, v))
        .find(|&(u, v)| u > 0 && v < 6)
        .expect("the fixture needs an edge with room on both sides");
    let verts = [u, v];
    let simplex = Simplex {
        diameter: sparse.get(u, v),
        index: rank(&bt, &verts),
    };
    for upper_only in [false, true] {
        counters::reset();
        let plain = cofacets(&sparse, &bt, simplex, &verts, 1, upper_only);
        let unbounded = counters::read().candidates;
        for bound in [sparse.max_distance(), f64::MAX, f64::INFINITY] {
            counters::reset();
            let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, upper_only, bound);
            let events = counters::read();
            assert_eq!(got, plain, "bits at the vacuous bound {bound}");
            assert_eq!(events.vacuous, 1, "raised bounds at {bound}");
            assert_eq!(
                events.candidates, unbounded,
                "entries read at the vacuous bound {bound}"
            );
        }
        // A bound under the largest stored distance keeps the test in
        // the merge.
        counters::reset();
        bounded_cofacets(
            &sparse,
            &bt,
            simplex,
            &verts,
            1,
            upper_only,
            positive_predecessor(sparse.max_distance()),
        );
        assert_eq!(
            counters::read().vacuous,
            0,
            "a bound that a distance reaches"
        );
    }
}

// The bound the engine passes is the simplex diameter, and a simplex of
// identical points has diameter zero. Both zeros are legal bounds there,
// and neither drops a cofacet at distance zero.
#[test]
fn a_zero_bound_keeps_the_cofacets_at_zero() {
    let triplets: Vec<(usize, usize, f64)> = (1..5)
        .flat_map(|i| (0..i).map(move |j| (i, j, 0.0)))
        .collect();
    let sparse = SparseDistanceMatrix::from_triplets(5, &triplets).unwrap();
    let dense = densify(&sparse);
    let bt = BinomialTable::new(5, 3).unwrap();
    let verts = [1usize, 2usize];
    let simplex = Simplex {
        diameter: 0.0,
        index: rank(&bt, &verts),
    };
    for upper_only in [false, true] {
        let expected = cofacets(&dense, &bt, simplex, &verts, 1, upper_only);
        for bound in [0.0f64, -0.0f64] {
            let got = bounded_cofacets(&dense, &bt, simplex, &verts, 1, upper_only, bound);
            assert_eq!(got, expected, "dense at bound {bound}");
            let got = bounded_cofacets(&sparse, &bt, simplex, &verts, 1, upper_only, bound);
            assert_eq!(got, expected, "sparse at bound {bound}");
        }
    }
}

// The dense source overrides the walk; `Counting` does not, so it runs
// the default body on the same distances. The two must agree in bits and
// in order, bounded and unbounded alike.
#[test]
fn the_dense_walk_matches_the_default_fold() {
    let mut rng = Rng::new(0x9e37_79b9);
    for n in [3usize, 5, 7] {
        for round in 0..4 {
            let (_, dense) = random_graph(&mut rng, n);
            let max_dim = if round % 2 == 0 { 1 } else { 2 };
            let bt = BinomialTable::new(n, max_dim + 2).unwrap();
            let plain = Counting {
                inner: &dense,
                reads: std::cell::Cell::new(0),
            };
            for (simplex, verts, dim) in simplices(&dense, &bt, max_dim) {
                for upper_only in [false, true] {
                    let expected = cofacets(&plain, &bt, simplex, &verts, dim, upper_only);
                    for square in [false, true] {
                        let form = if square {
                            dense.to_square()
                        } else {
                            dense.clone()
                        };
                        let got = cofacets(&form, &bt, simplex, &verts, dim, upper_only);
                        assert_eq!(
                            got, expected,
                            "n {n}, verts {verts:?}, upper {upper_only}, square {square}"
                        );
                        for bound in bounds(simplex, &expected) {
                            let want = bounded_cofacets(
                                &plain, &bt, simplex, &verts, dim, upper_only, bound,
                            );
                            let got = bounded_cofacets(
                                &form, &bt, simplex, &verts, dim, upper_only, bound,
                            );
                            assert_eq!(
                                got, want,
                                "n {n}, verts {verts:?}, upper {upper_only},                                      bound {bound}, square {square}"
                            );
                        }
                    }
                }
            }
        }
    }
}

// Both storage forms answer every query with the same bits.
#[test]
fn the_two_storage_forms_agree() {
    let mut rng = Rng::new(0x2f19_a7c3);
    let (_, condensed) = random_graph(&mut rng, 9);
    let square = condensed.to_square();
    assert!(
        !condensed.is_square(),
        "every constructor builds the compact form"
    );
    assert!(square.is_square());
    for i in 0..condensed.len() {
        for j in 0..condensed.len() {
            assert_eq!(
                square.get(i, j).to_bits(),
                condensed.get(i, j).to_bits(),
                "get({i}, {j})"
            );
        }
    }
    assert_eq!(
        square.enclosing_radius().to_bits(),
        condensed.enclosing_radius().to_bits(),
        "enclosing radius"
    );
    for threshold in [0.0, 1.0, 2.0, f64::INFINITY] {
        assert_eq!(
            square.count_edges_at(threshold),
            condensed.count_edges_at(threshold),
            "edges at {threshold}"
        );
        let a = square.to_sparse_at(threshold).unwrap();
        let b = condensed.to_sparse_at(threshold).unwrap();
        let edges = |m: &SparseDistanceMatrix| -> Vec<(usize, usize, u64)> {
            m.edges().map(|(u, v, d)| (u, v, d.to_bits())).collect()
        };
        assert_eq!(edges(&a), edges(&b), "graph at {threshold}");
    }
}

#[test]
fn scaled_norm_survives_extreme_magnitudes() {
    let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e200]]).unwrap();
    assert_eq!(d.get(0, 1), 1e200);
    let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e-200]]).unwrap();
    assert_eq!(d.get(0, 1), 1e-200);
    let d = DistanceMatrix::from_points(&[vec![3e200, 0.0], vec![0.0, 4e200]]).unwrap();
    assert!((d.get(0, 1) / 5e200 - 1.0).abs() < 1e-15);
}
