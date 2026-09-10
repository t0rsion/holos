use super::cofacet_support::graph;
use super::common::Rng;

use super::super::matrix::DistanceMatrix;
use super::super::sparse::SparseDistanceMatrix;

// The adversarial graphs, each one a shape that defeats a plausible
// enumerator shortcut.
pub(crate) fn adversarial_fixtures() -> Vec<(&'static str, SparseDistanceMatrix)> {
    let mut fixtures = vec![
        star_fixture(),
        joined_cliques_fixture(),
        bipartite_fixture(),
        all_equal_fixture(),
        duplicate_points_fixture(),
        skewed_fixture(),
        disconnected_fixture(),
    ];
    fixtures.extend(cut_fixtures());
    fixtures.push(complete_fixture());
    fixtures.push(("one point", graph(1, &[])));
    fixtures.push(("two points", graph(2, &[(0, 1, 1.0)])));
    fixtures.push(("edge across the range", graph(5, &[(0, 4, 1.0)])));
    fixtures
}

pub(crate) fn star_fixture() -> (&'static str, SparseDistanceMatrix) {
    let edges: Vec<_> = (1..7)
        .map(|vertex| (0, vertex, 1.0 + vertex as f64))
        .collect();
    ("star", graph(7, &edges))
}

pub(crate) fn joined_cliques_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut edges = Vec::new();
    for a in 0..4 {
        for b in 0..a {
            edges.push((a, b, 1.0));
            edges.push((a + 4, b + 4, 2.0));
        }
    }
    edges.push((3, 4, 3.0));
    ("joined cliques", graph(8, &edges))
}

pub(crate) fn bipartite_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut edges = Vec::new();
    for a in 0..3 {
        for b in 3..6 {
            edges.push((a, b, 1.0 + a as f64));
        }
    }
    ("bipartite", graph(6, &edges))
}

pub(crate) fn all_equal_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut edges = Vec::new();
    for a in 0..6 {
        for b in 0..a {
            edges.push((a, b, 2.0));
        }
    }
    ("all equal", graph(6, &edges))
}

pub(crate) fn duplicate_points_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut edges = Vec::new();
    for a in 0..6 {
        for b in 0..a {
            let distance = if a < 3 { 0.0 } else { 1.0 + b as f64 };
            edges.push((a, b, distance));
        }
    }
    ("duplicate points", graph(6, &edges))
}

pub(crate) fn skewed_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut edges = vec![(0, 1, 1.0), (0, 2, 1.0)];
    for a in 1..7 {
        for b in 1..a {
            if (a + b) % 3 != 0 {
                edges.push((a, b, 1.0 + (a * b) as f64 / 8.0));
            }
        }
    }
    ("least selective pivot", graph(7, &edges))
}

pub(crate) fn disconnected_fixture() -> (&'static str, SparseDistanceMatrix) {
    (
        "disconnected",
        graph(
            7,
            &[
                (0, 1, 1.0),
                (0, 2, 1.0),
                (1, 2, 1.0),
                (3, 4, 2.0),
                (3, 5, 2.0),
                (4, 5, 2.0),
            ],
        ),
    )
}

pub(crate) fn cut_fixtures() -> Vec<(&'static str, SparseDistanceMatrix)> {
    [
        ("threshold at the smallest edge", 1.0),
        ("threshold at a tie", 3.0),
        ("threshold between edge values", 2.5),
    ]
    .into_iter()
    .map(|(label, threshold)| (label, graph(7, &quantized_edges(threshold))))
    .collect()
}

pub(crate) fn quantized_edges(threshold: f64) -> Vec<(usize, usize, f64)> {
    let mut edges = Vec::new();
    for a in 0..7 {
        for b in 0..a {
            let distance = 1.0 + ((b * 7 + a) % 4) as f64;
            if distance <= threshold {
                edges.push((a, b, distance));
            }
        }
    }
    edges
}

pub(crate) fn complete_fixture() -> (&'static str, SparseDistanceMatrix) {
    let mut complete = Vec::new();
    for a in 0..7 {
        for b in 0..a {
            complete.push((a, b, 1.0 + ((a * 5 + b) % 4) as f64));
        }
    }
    ("dense as sparse", graph(7, &complete))
}

// A random sparse graph plus the dense matrix that uses +inf for every
// absent pair. The dense default then enumerates the same cofacets. It
// gives the missing ones an infinite diameter that the sparse side omits.
pub(crate) fn random_graph(rng: &mut Rng, n: usize) -> (SparseDistanceMatrix, DistanceMatrix) {
    // Duplicates and a zero so the diameter fold is exercised.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0];
    let mut triplets = Vec::new();
    let mut condensed = Vec::new();
    for i in 1..n {
        for j in 0..i {
            if rng.below(3) > 0 {
                let w = palette[rng.below(palette.len())];
                triplets.push((i, j, w));
                condensed.push(w);
            } else {
                condensed.push(f64::INFINITY);
            }
        }
    }
    (
        SparseDistanceMatrix::from_triplets(n, &triplets).unwrap(),
        DistanceMatrix::from_condensed(condensed).unwrap(),
    )
}

// A random graph with the density and the distance palette the caller
// asks for. `present` is the chance in a thousand that a pair is an
// edge, so a caller can reach a near-empty or a complete graph.
pub(crate) fn random_graph_shaped(
    rng: &mut Rng,
    n: usize,
    present: usize,
    palette: &[f64],
) -> SparseDistanceMatrix {
    let mut triplets = Vec::new();
    for i in 1..n {
        for j in 0..i {
            if rng.below(1000) < present {
                triplets.push((i, j, palette[rng.below(palette.len())]));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

// The sparse override must yield exactly what the dense default yields
// once its infinite-diameter (absent-neighbor) cofacets are dropped: the
// same indices, k, diameters, and descending order. The frozen reference
// stands between the two, so the shipped enumerator is compared against
// the body it replaced as well as against the dense one.
