use rustc_hash::FxHashMap;

use crate::field::Z2;
use crate::reduce::{Engine, Pivots};
use crate::simplex::Simplex;
use crate::{Diagram, DistanceMatrix, RipsParams};

/// Every in-complex edge, in column order: diameter descending, index
/// ascending. The reducer takes any such list, so a test can hand it one
/// without running the dim-0 pass first.
fn edge_columns(dist: &DistanceMatrix, engine: &Engine<'_, Z2, DistanceMatrix>) -> Vec<Simplex> {
    let mut columns = Vec::new();
    for i in 1..dist.len() {
        for j in 0..i {
            let diameter = dist.get(i, j);
            if engine.in_complex(diameter) {
                columns.push(Simplex {
                    diameter,
                    index: engine.bt.get(i, 2) + j as u64,
                });
            }
        }
    }
    columns.sort_unstable_by(|a, b| {
        b.diameter
            .total_cmp(&a.diameter)
            .then(a.index.cmp(&b.index))
    });
    columns
}

fn points(seed: u64, n: usize, coord_dim: usize) -> Vec<Vec<f64>> {
    let mut x = seed | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| (0..coord_dim).map(|_| next()).collect())
        .collect()
}

fn grid(side: usize) -> Vec<Vec<f64>> {
    (0..side)
        .flat_map(|a| (0..side).map(move |b| vec![a as f64, b as f64]))
        .collect()
}

/// The pivot registry the workers converge to must not depend on how many
/// of them there are. The check is wider than the diagram: it compares
/// the pivot index, the coefficient, the owning column, and the diameter
/// bits, and it compares the first three against the serial reducer as
/// well.
fn assert_registry_is_worker_invariant(dist: &DistanceMatrix, label: &str) {
    let mut serial_params = RipsParams::new(1);
    serial_params.threads = 1;
    let serial = Engine::new(dist, &serial_params, Z2).unwrap();
    let columns = edge_columns(dist, &serial);
    assert!(columns.len() > 64, "{label}: too few columns to be a gate");
    let empty: Pivots = FxHashMap::default();
    let mut diagram = Diagram::default();
    let want = serial.reduce_dimension(&columns, 1, &empty, &mut diagram);

    let mut first: Option<Vec<(u64, u64, usize, u64)>> = None;
    for budget in [2usize, 3, 4, 8] {
        let mut params = RipsParams::new(1);
        params.threads = budget;
        let engine = Engine::new(dist, &params, Z2).unwrap();
        let got = engine.parallel_pivot_registry(&columns, 1, &empty, budget);

        let by_index: Pivots = got
            .iter()
            .map(|&(index, coeff, col, _)| (index, (coeff, col)))
            .collect();
        assert_eq!(
            by_index, want,
            "{label}: {budget} workers disagree with the serial pivot registry"
        );
        match &first {
            None => first = Some(got),
            Some(want) => assert_eq!(
                &got, want,
                "{label}: {budget} workers disagree with 2 workers, diameter bits included"
            ),
        }
    }
}

#[test]
fn pivot_registry_is_worker_invariant_on_a_cloud() {
    let dist = DistanceMatrix::from_points(&points(20260818, 100, 3)).unwrap();
    assert_registry_is_worker_invariant(&dist, "cloud(n=100,d=3)");
}

#[test]
fn pivot_registry_is_worker_invariant_on_ties() {
    // A lattice puts many simplices at one diameter, which is where the
    // workers reorder the most and displace each other the most.
    let dist = DistanceMatrix::from_points(&grid(9)).unwrap();
    assert_registry_is_worker_invariant(&dist, "grid(9x9)");
}
