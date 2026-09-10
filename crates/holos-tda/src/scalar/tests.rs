use crate::*;

use super::execution::collapse_and_solve;
use super::routing::*;
use super::*;
use crate::collapse::verify::verify_dense;
use crate::distances;

fn grid(side: usize) -> DistanceMatrix {
    let mut points = Vec::new();
    for i in 0..side {
        for j in 0..side {
            points.push(vec![i as f64, j as f64]);
        }
    }
    DistanceMatrix::from_points(&points).unwrap()
}

fn bits(d: &Diagram) -> Vec<(usize, u64, u64)> {
    d.bars
        .iter()
        .map(|b| (b.dim, b.birth.to_bits(), b.death.to_bits()))
        .collect()
}

/// Run `f` and report how many conversions to the full storage form it
/// made. The counter is thread-local and each test owns its thread.
fn counting<T>(f: impl FnOnce() -> T) -> (usize, T) {
    distances::SQUARE_BUILDS.with(|c| c.set(0));
    let value = f();
    (distances::SQUARE_BUILDS.with(|c| c.get()), value)
}

// The frozen rule, pinned against its constants. A change to either
// constant fails here first, before it reaches a measurement.
#[test]
fn the_routing_rule_holds_its_constants() {
    assert!(!may_route(N_MIN - 1, 1.0), "under N_MIN nothing routes");
    assert!(may_route(N_MIN, 1.0));
    assert!(
        may_route(N_MIN, f64::INFINITY),
        "an infinite threshold routes"
    );
    for bad in [f64::NAN, -1.0] {
        assert!(!may_route(N_MIN, bad), "threshold {bad} must not route");
    }
    // The last accepted and the first rejected edge count, to the
    // edge. Four fifths of C(n, 2) is not always an integer, so the
    // cutoff can fall between two counts, and each case names the two
    // counts it falls between.
    for (n, last_routed) in [
        (N_MIN, 396usize),
        (33, 422),
        (100, 3960),
        (1001, 400_400),
        (4000, 6_398_400),
    ] {
        let pairs = n * (n - 1) / 2;
        assert_eq!(last_routed, 4 * pairs / 5, "{n}: the cutoff moved");
        assert!(density_routes(n, 0), "{n}: an empty graph routes");
        assert!(density_routes(n, last_routed), "{n}: the cutoff routes");
        assert!(!density_routes(n, last_routed + 1), "{n}: one edge over");
        assert!(!density_routes(n, pairs), "{n}: a complete graph");
    }
}

// The budget, at the byte, in both regimes. Below 2897 points the
// matrix is under 32 MiB and the floor decides; above it the matrix
// does. The last edge that fits is `(budget - 24 n - 8) / 24`.
#[test]
fn the_conversion_budget_gates_at_the_byte() {
    let small = 1000;
    assert!(pair_count(small) * 8 < MIN_CONVERSION_BYTES);
    let last_fit = ((MIN_CONVERSION_BYTES - 24 * small as u128 - 8) / 24) as usize;
    assert_eq!(last_fit, 1_397_101);
    assert!(memory_routes(small, last_fit));
    assert!(!memory_routes(small, last_fit + 1));

    let large = 20_000;
    let budget = pair_count(large) * 8;
    assert!(budget > MIN_CONVERSION_BYTES);
    let last_fit = ((budget - 24 * large as u128 - 8) / 24) as usize;
    assert_eq!(last_fit, 66_643_333);
    assert!(memory_routes(large, last_fit));
    assert!(!memory_routes(large, last_fit + 1));
}

// A near-clique the density cutoff alone would route: 2000 points with
// three quarters of the pairs at or below the threshold. The graph
// keeps over a million edges and about 36 MB against a 32 MiB budget,
// so Auto refuses it. The test counts the edges of a real matrix and
// stops at the decision, because the reduction itself is not cheap.
#[test]
fn a_large_near_clique_stays_dense() {
    let n = 2000;
    let data: Vec<f64> = (0..n * (n - 1) / 2)
        .map(|k| if k % 4 == 0 { 3.0 } else { 1.0 })
        .collect();
    let dist = DistanceMatrix::from_condensed(data).unwrap();
    let threshold = 1.0;
    let edges = dist.count_edges_at(threshold);
    assert!(edges > 1_000_000, "{edges} edges");
    assert!(may_route(n, threshold));
    assert!(density_routes(n, edges), "the density cutoff accepts it");
    assert!(!memory_routes(n, edges), "the budget must refuse it");
    assert!(!graph_routes(n, edges));
}

// The frozen storage rule, pinned against its constants, the same way
// the routing rule is.
#[test]
fn the_storage_rule_holds_its_constants() {
    assert!(!square_size_fits(1024), "1024 points stay compact");
    assert!(
        square_size_fits(1025),
        "1025 points may take both triangles"
    );
    assert!(square_size_fits(8191), "8191 points fit the byte budget");
    assert!(!square_size_fits(8192), "8192 points exceed it");
    assert_eq!(square_extra_bytes(8191), 268_402_688);

    // The work test is `edges * max_dim >= (READS_PER_CELL - 1) * n`:
    // the dim-0 walk over the whole matrix contributes one read per
    // cell on its own, and the rest has to come from the columns above
    // it.
    for n in [1025usize, 2400, 8191] {
        let last_refused = (SQUARE_READS_PER_CELL as usize - 1) * n - 1;
        assert!(!square_work_pays(n, last_refused, 1), "{n}: one edge under");
        assert!(
            square_work_pays(n, last_refused + 1, 1),
            "{n}: at the cutoff"
        );
        // A second dimension doubles the reads, so half the edges do.
        assert!(
            square_work_pays(n, last_refused / 2 + 1, 2),
            "{n}: max_dim 2"
        );
    }
    assert!(!square_work_pays(2400, 863, 1), "a low threshold refuses");
    assert!(
        square_work_pays(2400, 14_273, 1),
        "a sparse block graph pays"
    );
}

// The storage form follows the routing decision. A routed run holds no
// distance matrix at all, so it builds no full form, not even when the
// caller forces one. The fixture asserts that the rule would have
// selected the full form, so it cannot stop testing the interaction
// when a constant moves.
#[test]
fn a_routed_run_never_builds_the_full_form() {
    let n = 1030;
    let dist = band(n, 4);
    let threshold = resolved_threshold(&dist, &RipsParams::new(1));
    let edges = dist.count_edges_at(threshold);
    assert!(
        may_route(n, threshold) && graph_routes(n, edges),
        "the fixture must route: {edges} edges"
    );
    assert!(
        square_size_fits(n) && square_work_pays(n, edges, 1),
        "the storage rule must want the full form here"
    );

    let params = RipsParams::new(1);
    let mut reference = None;
    for storage in [
        DenseStorage::Auto,
        DenseStorage::Compact,
        DenseStorage::Square,
    ] {
        let p = params.clone().with_dense_storage(storage);
        let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
        assert_eq!(built, 0, "{storage:?}: a routed run stays compact");
        let bits = bits(&diagram);
        assert_eq!(*reference.get_or_insert(bits.clone()), bits, "{storage:?}");
    }

    // The same input on the dense engine, where the rule does decide.
    for (storage, want) in [
        (DenseStorage::Auto, 1),
        (DenseStorage::Compact, 0),
        (DenseStorage::Square, 1),
    ] {
        let p = params
            .clone()
            .with_engine(Engine::Dense)
            .with_dense_storage(storage);
        let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
        assert_eq!(built, want, "{storage:?}: conversions");
        assert_eq!(bits(&diagram), *reference.as_ref().unwrap(), "{storage:?}");
    }
}

// Below the size bound `Auto` keeps the compact form, `Square` still
// converts, and every form gives one diagram. The matrix is small, so
// this covers the whole engine and storage product cheaply.
#[test]
fn every_storage_form_gives_one_diagram() {
    let dist = grid(6);
    assert!(
        !square_size_fits(dist.len()),
        "the fixture is under the bound"
    );
    let params = RipsParams::new(2);
    let mut reference = None;
    for engine in [Engine::Auto, Engine::Dense, Engine::Sparse] {
        for storage in [
            DenseStorage::Auto,
            DenseStorage::Compact,
            DenseStorage::Square,
        ] {
            let p = params
                .clone()
                .with_engine(engine)
                .with_dense_storage(storage);
            let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
            let dense_run = engine == Engine::Dense
                || (engine == Engine::Auto
                    && !graph_routes(
                        dist.len(),
                        dist.count_edges_at(resolved_threshold(&dist, &p)),
                    ));
            let want = usize::from(dense_run && storage == DenseStorage::Square);
            assert_eq!(built, want, "{engine:?}, {storage:?}: conversions");
            let bits = bits(&diagram);
            assert_eq!(
                *reference.get_or_insert(bits.clone()),
                bits,
                "{engine:?}, {storage:?}"
            );
        }
    }
}

/// A band matrix: `d(i, j)` is `|i - j|` within `width` and absent
/// outside it. Every row holds an absent pair, so the enclosing radius
/// is infinite and the graph stays sparse at any threshold.
fn band(n: usize, width: usize) -> DistanceMatrix {
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            data.push(if i - j <= width {
                (i - j) as f64
            } else {
                f64::INFINITY
            });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

// A dense matrix of mostly absent pairs has a sparse graph even with no
// threshold, and its enclosing radius is infinite, so the default and
// the explicit infinite threshold are the same run. A matrix with no
// absent pair keeps every pair and stays dense.
#[test]
fn an_infinite_threshold_routes_on_density() {
    let n = 40;
    let dist = band(n, 3);
    assert_eq!(dist.enclosing_radius(), f64::INFINITY);
    let edges = dist.count_edges_at(f64::INFINITY);
    assert_eq!(edges, 3 * n - 6, "the band holds its own edges");
    assert!(may_route(n, f64::INFINITY) && graph_routes(n, edges));

    let complete = grid(7);
    let n = complete.len();
    let all = complete.count_edges_at(f64::INFINITY);
    assert_eq!(all, n * (n - 1) / 2, "every pair of a grid is finite");
    assert!(!graph_routes(n, all), "a complete matrix stays dense");

    // The routed diagram, against the dense one, on both spellings of
    // the infinite threshold.
    let params = RipsParams::new(2);
    let dense = rips_persistence(&dist, &params.clone().with_engine(Engine::Dense)).unwrap();
    for threshold in [None, Some(f64::INFINITY)] {
        let mut p = params.clone();
        p.threshold = threshold;
        for engine in [Engine::Auto, Engine::Sparse] {
            let got = rips_persistence(&dist, &p.clone().with_engine(engine)).unwrap();
            assert_eq!(bits(&got), bits(&dense), "{threshold:?}, {engine:?}");
        }
    }
}

// The routed path against the dense one on an input the rule accepts.
// The fixture asserts its own routing, so it cannot quietly stop
// testing the conversion when a constant moves.
#[test]
fn a_routed_input_gives_the_dense_diagram() {
    let side = (N_MIN as f64).sqrt().ceil() as usize + 1;
    let dist = grid(side);
    let threshold = 1.5;
    let edges = dist.count_edges_at(threshold);
    assert!(
        may_route(dist.len(), threshold) && graph_routes(dist.len(), edges),
        "the fixture must route: {} points, {edges} edges",
        dist.len()
    );
    let params = RipsParams::new(1).with_threshold(threshold);
    let dense = rips_persistence(&dist, &params.clone().with_engine(Engine::Dense)).unwrap();
    for engine in [Engine::Auto, Engine::Sparse] {
        let got = rips_persistence(&dist, &params.clone().with_engine(engine)).unwrap();
        assert_eq!(bits(&got), bits(&dense), "{engine:?}");
    }
}

// The default threshold of a dense input is its enclosing radius, and
// of a sparse one is no threshold at all. Routing must carry the dense
// default across, or the routed filtration would be larger.
#[test]
fn routing_carries_the_dense_default_threshold() {
    let side = (N_MIN as f64).sqrt().ceil() as usize + 1;
    let dist = grid(side);
    let radius = dist.enclosing_radius();
    let params = RipsParams::new(1);
    let auto = rips_persistence(&dist, &params).unwrap();
    let explicit = rips_persistence(
        &dist,
        &params
            .clone()
            .with_engine(Engine::Sparse)
            .with_threshold(radius),
    )
    .unwrap();
    assert_eq!(bits(&auto), bits(&explicit));
    // Every pair of this grid is finite, so no threshold at all would
    // admit strictly more edges than the enclosing radius does.
    assert!(dist.count_edges_at(radius) < dist.len() * (dist.len() - 1) / 2);
}

#[test]
fn pipeline_runs_the_selected_schedule() {
    // The diagram is the same under every schedule, so only the
    // certificate the pipeline hands to `report` shows which collapse
    // ran: version 2 for rounds, version 1 otherwise, and the ordered
    // run at four workers tests more edges than the serial one on this
    // grid. Every certificate must pass the independent verifier.
    let dist = grid(5);
    let plain = rips_persistence(&dist, &RipsParams::new(2)).unwrap();
    let mut seen = Vec::new();
    for schedule in [
        CollapseSchedule::Serial,
        CollapseSchedule::Ordered,
        CollapseSchedule::Rounds,
        CollapseSchedule::Adaptive,
    ] {
        let params = RipsParams::new(2)
            .with_threads(4)
            .with_collapse_schedule(schedule);
        let mut captured = None;
        let diagram = collapse_and_solve(&dist, &params, |c| {
            captured = Some(c.clone());
            Ok(())
        })
        .unwrap();
        let captured = captured.expect("report must see the collapse");
        verify_dense(&dist, None, &captured).unwrap();
        let expected_version = match schedule {
            CollapseSchedule::Serial | CollapseSchedule::Ordered => 1,
            CollapseSchedule::Rounds => 2,
            CollapseSchedule::Adaptive => 3,
        };
        assert_eq!(captured.certificate.algorithm_version(), expected_version);
        let mut a = diagram.clone();
        let mut b = plain.clone();
        a.canonicalize();
        b.canonicalize();
        assert_eq!(a.bars, b.bars, "{schedule:?}");
        seen.push((schedule, captured.stats));
    }
    let serial = seen[0].1;
    let ordered = seen[1].1;
    assert_eq!(ordered.logical_tests, serial.edge_tests);
    assert!(
        ordered.edge_tests > serial.edge_tests,
        "the ordered schedule did not speculate: {} vs {}",
        ordered.edge_tests,
        serial.edge_tests
    );
    assert_eq!(serial.window_batches, 0);
    assert!(ordered.window_batches > 0);
}
