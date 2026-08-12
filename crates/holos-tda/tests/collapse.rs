//! Edge collapse gates: bar-for-bar equivalence with the uncollapsed engine,
//! certificate properties, adversarial fixtures, and the public API surface.
//!
//! The collapse is preprocessing, so no bar may move by one bit. Every
//! diagram comparison here is exact on canonicalized bars, with no tolerance.
//! Small fixtures also face the brute-force oracle.
//!
//! The hand-built fixtures name the property they attack. Their edge sets are
//! chosen against the frozen schedule (decreasing value, ties by combinadic
//! index) and the frozen witness rule, so the expected certificates below are
//! derived from the specification, not observed from a run.

use holos_tda::collapse::verify::{verify_dense, verify_sparse};
use holos_tda::collapse::{collapse_dense, collapse_sparse, CollapsedRips, RemovalStep};
use holos_tda::oracle::rips_persistence_oracle_mod;
use holos_tda::{
    rips_persistence, rips_persistence_sparse, Bar, Diagram, DistanceMatrix, RipsParams,
    SparseDistanceMatrix,
};

const MODULI: [u32; 3] = [2, 3, 5];
const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];
const ALL_ON: (bool, bool, bool) = (true, true, true);
const ALL_OFF: (bool, bool, bool) = (false, false, false);
/// Every clearing x emergent-pairs x apparent-pairs combination.
const TOGGLE_CROSS: [(bool, bool, bool); 8] = [
    (false, false, false),
    (false, false, true),
    (false, true, false),
    (false, true, true),
    (true, false, false),
    (true, false, true),
    (true, true, false),
    (true, true, true),
];

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

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn params(
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> RipsParams {
    let mut p = RipsParams::new(max_dim)
        .with_modulus(modulus)
        .with_threads(threads);
    p.threshold = threshold;
    p.use_clearing = toggles.0;
    p.use_emergent_pairs = toggles.1;
    p.use_apparent_pairs = toggles.2;
    if collapse {
        p = p.with_edge_collapse();
    }
    p
}

fn canon(diagram: &Diagram) -> Vec<Bar> {
    let mut d = diagram.clone();
    d.canonicalize();
    d.bars
}

fn dense_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence(dist, &p).unwrap())
}

fn sparse_bars(
    dist: &SparseDistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence_sparse(dist, &p).unwrap())
}

fn oracle_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> Vec<Bar> {
    canon(&rips_persistence_oracle_mod(
        dist, max_dim, threshold, modulus,
    ))
}

fn essential_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_infinite())
        .count()
}

fn finite_count(bars: &[Bar], dim: usize) -> usize {
    bars.iter()
        .filter(|b| b.dim == dim && b.death.is_finite())
        .count()
}

/// Dense matrix from an explicit edge list. Every unlisted pair is +inf.
fn dense_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> DistanceMatrix {
    let mut full = vec![f64::INFINITY; n * n];
    for &(u, v, d) in edges {
        full[u * n + v] = d;
        full[v * n + u] = d;
    }
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(full[i * n + j]);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

fn sparse_from_edges(n: usize, edges: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(n, edges).unwrap()
}

/// The sparse form of a dense matrix: every finite entry becomes an edge.
fn sparse_from_dense(dist: &DistanceMatrix) -> SparseDistanceMatrix {
    let n = dist.len();
    let mut triplets = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dist.get(i, j);
            if d.is_finite() {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
}

fn edge_list(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
    matrix.edges().collect()
}

/// The edges the collapse must describe: finite, at or below the resolved
/// threshold, in ascending endpoint order.
fn thresholded_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> Vec<(usize, usize, f64)> {
    let t = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let n = dist.len();
    let mut out = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let d = dist.get(u, v);
            if d.is_finite() && d <= t {
                out.push((u, v, d));
            }
        }
    }
    out
}

fn thresholded_sparse(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Vec<(usize, usize, f64)> {
    let t = threshold.unwrap_or(f64::INFINITY);
    dist.edges().filter(|&(_, _, d)| d <= t).collect()
}

fn removed_edges(result: &CollapsedRips) -> Vec<(usize, usize, f64)> {
    let mut v: Vec<(usize, usize, f64)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| {
            let (u, v) = s.edge();
            (u, v, s.value())
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    v
}

fn step_for(result: &CollapsedRips, edge: (usize, usize)) -> Option<&RemovalStep> {
    result.certificate.steps().iter().find(|s| s.edge() == edge)
}

/// Header, partition, and witness-shape checks that do not depend on the
/// input kind. `resolved` is the threshold after the input's own rule.
fn check_certificate(
    name: &str,
    n: usize,
    input: &[(usize, usize, f64)],
    threshold: Option<f64>,
    resolved: f64,
    result: &CollapsedRips,
) {
    let cert = &result.certificate;
    let output = edge_list(&result.matrix);

    assert_eq!(cert.algorithm_version(), 1, "{name}: algorithm version");
    assert_eq!(cert.vertex_count(), n, "{name}: vertex count");
    assert_eq!(result.matrix.len(), n, "{name}: output vertex count");
    assert_eq!(
        cert.requested_threshold(),
        threshold,
        "{name}: requested threshold must be verbatim"
    );

    let terminal = if resolved.is_finite() {
        resolved
    } else {
        input.iter().map(|e| e.2).fold(0.0, f64::max)
    };
    assert_eq!(cert.terminal_level(), terminal, "{name}: terminal level");

    assert_eq!(cert.input_edge_count(), input.len(), "{name}: input edges");
    assert_eq!(
        cert.output_edge_count(),
        output.len(),
        "{name}: output edges"
    );
    assert_eq!(
        cert.input_edge_count(),
        cert.output_edge_count() + cert.steps().len(),
        "{name}: input must equal output plus steps"
    );

    // Removed and surviving edges partition the thresholded input, values
    // included. A duplicate or an altered value breaks this compare.
    let mut merged = output.clone();
    merged.extend(removed_edges(result));
    merged.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    assert_eq!(
        merged, input,
        "{name}: output plus removals must reconstruct the thresholded input"
    );

    for &(u, v, d) in &output {
        assert!(u < v, "{name}: output edge ({u}, {v}) not ordered");
        assert!(
            d <= terminal,
            "{name}: output edge ({u}, {v}) above terminal"
        );
    }

    let mut last_pass = 0;
    for (i, step) in cert.steps().iter().enumerate() {
        let (u, v) = step.edge();
        assert!(u < v, "{name}: step {i} endpoints not ordered");
        assert!(v < n, "{name}: step {i} endpoint out of range");
        assert!(step.pass() >= 1, "{name}: step {i} pass is not 1-based");
        assert!(
            step.pass() >= last_pass,
            "{name}: step {i} pass number decreased"
        );
        last_pass = step.pass();

        let w = step.witnesses();
        assert!(!w.is_empty(), "{name}: step {i} has no witness segment");
        assert_eq!(
            w[0].0,
            step.value(),
            "{name}: step {i} first segment must start at the edge value"
        );
        for pair in w.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "{name}: step {i} segment starts must strictly increase"
            );
        }
        for &(start, apex) in w {
            assert!(apex < n, "{name}: step {i} apex {apex} out of range");
            assert!(
                apex != u && apex != v,
                "{name}: step {i} apex is an endpoint"
            );
            assert!(
                start <= terminal,
                "{name}: step {i} segment starts above the terminal level"
            );
        }
    }

    assert_eq!(
        result.stats.input_edges,
        cert.input_edge_count(),
        "{name}: stats input_edges"
    );
    assert_eq!(
        result.stats.output_edges,
        cert.output_edge_count(),
        "{name}: stats output_edges"
    );
    assert_eq!(
        result.stats.removed_edges,
        cert.steps().len(),
        "{name}: stats removed_edges"
    );
    assert!(result.stats.passes >= 1, "{name}: stats passes");
    assert_eq!(
        result.stats.witness_segments,
        cert.steps()
            .iter()
            .map(|s| s.witnesses().len())
            .sum::<usize>(),
        "{name}: stats witness_segments"
    );
    if let Some(last) = cert.steps().last() {
        assert!(
            result.stats.passes >= last.pass(),
            "{name}: stats passes must cover the last removal"
        );
    }
}

/// Collapse a dense input and run every input-independent certificate check:
/// partition, subset with unchanged values, the independent verifier,
/// idempotence, and a byte-identical rerun.
fn collapse_and_check_dense(
    name: &str,
    dist: &DistanceMatrix,
    threshold: Option<f64>,
) -> CollapsedRips {
    let result = collapse_dense(dist, threshold).unwrap();
    let resolved = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let input = thresholded_dense(dist, threshold);
    check_certificate(name, dist.len(), &input, threshold, resolved, &result);

    for (u, v, d) in edge_list(&result.matrix) {
        assert_eq!(
            dist.get(u, v),
            d,
            "{name}: surviving edge ({u}, {v}) changed value"
        );
    }
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert_eq!(
            dist.get(u, v),
            step.value(),
            "{name}: step for ({u}, {v}) changed value"
        );
    }

    verify_dense(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));

    check_idempotent(name, &result);

    let rerun = collapse_dense(dist, threshold).unwrap();
    assert_eq!(
        rerun.certificate, result.certificate,
        "{name}: rerun changed the certificate"
    );
    assert_eq!(
        edge_list(&rerun.matrix),
        edge_list(&result.matrix),
        "{name}: rerun changed the output matrix"
    );
    result
}

fn collapse_and_check_sparse(
    name: &str,
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> CollapsedRips {
    let result = collapse_sparse(dist, threshold).unwrap();
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let input = thresholded_sparse(dist, threshold);
    check_certificate(name, dist.len(), &input, threshold, resolved, &result);

    for (u, v, d) in edge_list(&result.matrix) {
        assert_eq!(
            dist.get(u, v),
            d,
            "{name}: surviving edge ({u}, {v}) changed value"
        );
    }

    verify_sparse(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));

    check_idempotent(name, &result);

    let rerun = collapse_sparse(dist, threshold).unwrap();
    assert_eq!(
        rerun.certificate, result.certificate,
        "{name}: rerun changed the certificate"
    );
    assert_eq!(
        edge_list(&rerun.matrix),
        edge_list(&result.matrix),
        "{name}: rerun changed the output matrix"
    );
    result
}

/// Collapsing the collapsed graph must remove nothing in a single pass.
fn check_idempotent(name: &str, result: &CollapsedRips) {
    let terminal = Some(result.certificate.terminal_level());
    let again = collapse_sparse(&result.matrix, terminal).unwrap();
    assert!(
        again.certificate.steps().is_empty(),
        "{name}: collapsing the collapsed graph removed {} edges",
        again.certificate.steps().len()
    );
    assert_eq!(
        again.stats.passes, 1,
        "{name}: idempotent run needs one pass"
    );
    assert_eq!(
        again.stats.removed_edges, 0,
        "{name}: idempotent run removed edges"
    );
    assert_eq!(
        edge_list(&again.matrix),
        edge_list(&result.matrix),
        "{name}: idempotent run changed the graph"
    );
    verify_sparse(&result.matrix, terminal, &again)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the idempotent run: {e}"));
}

/// The equality battery: collapse on and off must agree bar for bar over the
/// modulus x threshold x threads x max_dim x toggles cross, on the dense input
/// and on its sparse equivalent. The toggle arm is the full eight-way cross at
/// threads {1, 4} and the two extremes at the other thread counts. Small
/// inputs also face the oracle.
fn assert_collapse_preserves_diagram(
    name: &str,
    dense: &DistanceMatrix,
    mid: f64,
    check_oracle: bool,
) {
    let sparse = sparse_from_dense(dense);
    let thresholds = [None, Some(mid), Some(f64::INFINITY)];
    for &modulus in &MODULI {
        for threshold in thresholds {
            for &threads in &THREAD_COUNTS {
                // Toggle coverage: the full eight-way cross at one and four
                // threads, the two extremes at every thread count. The
                // crossed pair is modulus x threshold x max_dim x toggles at
                // threads {1, 4}, and modulus x threshold x max_dim x
                // {all on, all off} at threads {1, 2, 4, 8}.
                let toggle_set: &[(bool, bool, bool)] = match threads {
                    1 | 4 => &TOGGLE_CROSS,
                    _ => &[ALL_ON, ALL_OFF],
                };
                for max_dim in 0..=2 {
                    for &toggles in toggle_set {
                        let label = format!(
                            "{name}: p={modulus} threshold={threshold:?} threads={threads} \
                             max_dim={max_dim} toggles={toggles:?}"
                        );
                        let plain =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, false);
                        let collapsed =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, true);
                        assert_eq!(plain, collapsed, "{label}: dense");

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let collapsed = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, collapsed, "{label}: sparse");
                    }
                }
            }
            if check_oracle {
                let collapsed = dense_bars(dense, 2, threshold, modulus, 1, ALL_ON, true);
                assert_eq!(
                    collapsed,
                    oracle_bars(dense, 2, threshold, modulus),
                    "{name}: collapsed diagram differs from the oracle \
                     (p={modulus} threshold={threshold:?})"
                );
            }
        }
    }
}

/// Per-fixture gate: full certificate checks plus collapse-on against
/// collapse-off at several fields and thread counts, dense and sparse.
fn assert_fixture(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    max_dim: usize,
    check_oracle: bool,
) -> CollapsedRips {
    let result = collapse_and_check_dense(name, dense, threshold);
    let sparse = sparse_from_dense(dense);
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(dense, max_dim, threshold, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(dense, max_dim, threshold, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "{name}: dense collapse changed the diagram (p={modulus} threads={threads})"
            );
            let plain_sparse =
                sparse_bars(&sparse, max_dim, threshold, modulus, threads, ALL_ON, false);
            let collapsed_sparse =
                sparse_bars(&sparse, max_dim, threshold, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain_sparse, collapsed_sparse,
                "{name}: sparse collapse changed the diagram (p={modulus} threads={threads})"
            );
            if check_oracle && threads == 1 {
                assert_eq!(
                    collapsed,
                    oracle_bars(dense, max_dim, threshold, modulus),
                    "{name}: collapsed diagram differs from the oracle (p={modulus})"
                );
            }
        }
    }
    result
}

// Battery inputs. Each one is small enough for the oracle at max_dim 2.

// Generic point cloud: distinct values, no ties, no absent edges.
fn battery_points() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    DistanceMatrix::from_points(&points).unwrap()
}

// Two-value palette: every comparison in the predicate meets a tie.
fn battery_ties() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d002);
    let palette = [1.0, 2.0];
    let condensed: Vec<f64> = (0..6 * 5 / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}

// Coincident points: zero-value edges enter at the bottom of the filtration.
fn battery_zeros() -> DistanceMatrix {
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.5, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        points.push(site.to_vec());
        points.push(site.to_vec());
    }
    DistanceMatrix::from_points(&points).unwrap()
}

// Absent pairs inside a connected graph: the collapse must treat +inf as a
// missing edge, never as a large one.
fn battery_infinite() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d003);
    let palette = [1.0, 2.0, 3.0];
    let condensed: Vec<f64> = (0..7 * 6 / 2)
        .map(|_| {
            if rng.uniform() < 0.3 {
                f64::INFINITY
            } else {
                palette[rng.below(palette.len())]
            }
        })
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}

// Three components at different scales: the enclosing radius is +inf, so the
// terminal level falls back to the largest finite edge.
fn battery_disconnected() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (3, 4, 2.0),
        (3, 5, 2.0),
        (4, 5, 2.0),
        (6, 7, 1.0),
        (6, 8, 2.0),
        (7, 8, 2.0),
    ];
    dense_from_edges(9, &edges)
}

#[test]
fn collapse_preserves_the_diagram_on_a_point_cloud() {
    assert_collapse_preserves_diagram("points", &battery_points(), 0.7, true);
}

#[test]
fn collapse_preserves_the_diagram_with_ties() {
    assert_collapse_preserves_diagram("ties", &battery_ties(), 1.0, true);
}

#[test]
fn collapse_preserves_the_diagram_with_zero_distances() {
    assert_collapse_preserves_diagram("zeros", &battery_zeros(), 0.6, true);
}

#[test]
fn collapse_preserves_the_diagram_with_absent_edges() {
    assert_collapse_preserves_diagram("infinite", &battery_infinite(), 2.0, true);
}

#[test]
fn collapse_preserves_the_diagram_when_disconnected() {
    assert_collapse_preserves_diagram("disconnected", &battery_disconnected(), 1.5, true);
}

// Certificate properties.

#[test]
fn certificate_properties_hold_on_every_battery_input() {
    let cases: [(&str, DistanceMatrix, f64); 5] = [
        ("points", battery_points(), 0.7),
        ("ties", battery_ties(), 1.0),
        ("zeros", battery_zeros(), 0.6),
        ("infinite", battery_infinite(), 2.0),
        ("disconnected", battery_disconnected(), 1.5),
    ];
    for (name, dense, mid) in cases {
        let sparse = sparse_from_dense(&dense);
        for threshold in [None, Some(mid), Some(f64::INFINITY)] {
            let label = format!("{name} threshold={threshold:?}");
            collapse_and_check_dense(&label, &dense, threshold);
            collapse_and_check_sparse(&label, &sparse, threshold);
        }
    }
}

#[test]
fn dense_and_sparse_forms_agree_on_one_graph() {
    // Same graph, same explicit threshold: the two entry points must produce
    // the same schedule, the same witnesses, and the same output matrix.
    let dense = level_dependent_apex_matrix();
    let sparse = sparse_from_dense(&dense);
    let threshold = Some(2.0);
    let from_dense = collapse_dense(&dense, threshold).unwrap();
    let from_sparse = collapse_sparse(&sparse, threshold).unwrap();
    assert_eq!(
        from_dense.certificate, from_sparse.certificate,
        "dense and sparse certificates differ"
    );
    assert_eq!(
        edge_list(&from_dense.matrix),
        edge_list(&from_sparse.matrix),
        "dense and sparse output matrices differ"
    );
    assert_eq!(
        from_dense.stats, from_sparse.stats,
        "dense and sparse stats differ"
    );
}

fn scale_dense(dist: &DistanceMatrix, factor: f64) -> DistanceMatrix {
    let n = dist.len();
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(factor * dist.get(i, j));
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

#[test]
fn positive_scaling_preserves_removals_and_scales_witnesses() {
    // Scaling by 3 is exact on these values, so the schedule order and every
    // comparison in the predicate survive unchanged. Only the recorded
    // breakpoints move, by the same factor.
    let dense = level_dependent_apex_matrix();
    let scaled = scale_dense(&dense, 3.0);
    let base = collapse_and_check_dense("scaling base", &dense, Some(2.0));
    let big = collapse_and_check_dense("scaling scaled", &scaled, Some(6.0));

    assert_eq!(
        big.certificate.terminal_level(),
        3.0 * base.certificate.terminal_level(),
        "terminal level must scale"
    );
    let base_steps = base.certificate.steps();
    let big_steps = big.certificate.steps();
    assert_eq!(
        base_steps.len(),
        big_steps.len(),
        "scaling changed the number of removals"
    );
    for (i, (a, b)) in base_steps.iter().zip(big_steps).enumerate() {
        assert_eq!(a.edge(), b.edge(), "step {i}: scaling changed the edge");
        assert_eq!(a.pass(), b.pass(), "step {i}: scaling changed the pass");
        assert_eq!(
            b.value(),
            3.0 * a.value(),
            "step {i}: value must scale exactly"
        );
        assert_eq!(
            a.witnesses().len(),
            b.witnesses().len(),
            "step {i}: scaling changed the segment count"
        );
        for (j, (wa, wb)) in a.witnesses().iter().zip(b.witnesses()).enumerate() {
            assert_eq!(wa.1, wb.1, "step {i} segment {j}: apex changed");
            assert_eq!(
                wb.0,
                3.0 * wa.0,
                "step {i} segment {j}: start must scale exactly"
            );
        }
    }
}

fn permute_dense(dist: &DistanceMatrix, perm: &[usize]) -> DistanceMatrix {
    let n = dist.len();
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(dist.get(perm[i], perm[j]));
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

#[test]
fn vertex_permutation_preserves_the_barcode_only() {
    // The reduced graph is not canonical, so nothing is asserted about which
    // edges survive a relabeling. The barcode is the invariant.
    let dense = level_dependent_apex_matrix();
    let mut rng = Rng::new(0x9e37_79b9);
    let n = dense.len();
    let mut perm: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        perm.swap(i, rng.below(i + 1));
    }
    let permuted = permute_dense(&dense, &perm);
    collapse_and_check_dense("permuted", &permuted, Some(2.0));
    for &modulus in &MODULI {
        let base = dense_bars(&dense, 2, Some(2.0), modulus, 1, ALL_ON, true);
        let moved = dense_bars(&permuted, 2, Some(2.0), modulus, 1, ALL_ON, true);
        assert_eq!(base, moved, "permutation changed the barcode (p={modulus})");
    }
}

#[test]
fn empty_graph_collapses_to_nothing() {
    // No edges at all: the terminal level falls back to 0 and the schedule
    // still runs exactly one pass.
    let empty = sparse_from_edges(5, &[]);
    let result = collapse_and_check_sparse("empty", &empty, None);
    assert_eq!(result.certificate.terminal_level(), 0.0, "terminal level");
    assert_eq!(result.certificate.input_edge_count(), 0, "input edges");
    assert_eq!(result.stats.passes, 1, "passes");
    assert_eq!(result.stats.max_common_neighborhood, 0, "neighborhood");
}

// Named adversarial fixtures.

/// A single fixed apex cannot certify edge (0, 1): vertex 2 is the only
/// candidate at level 1, but it is not adjacent to vertex 3, which joins the
/// candidate set at level 2. Vertex 4 covers the upper level. Vertices 5
/// through 9 are private blockers: each one keeps a value-2 edge alive
/// through the first sweep, so the candidate set of (0, 1) is still complete
/// when the schedule reaches it.
fn level_dependent_apex_matrix() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (2, 4, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (3, 4, 2.0),
        (0, 5, 2.0),
        (3, 5, 2.0),
        (1, 6, 2.0),
        (3, 6, 2.0),
        (0, 7, 2.0),
        (4, 7, 2.0),
        (1, 8, 2.0),
        (4, 8, 2.0),
        (3, 9, 2.0),
        (4, 9, 2.0),
    ];
    dense_from_edges(10, &edges)
}

#[test]
fn level_dependent_apex() {
    let dense = level_dependent_apex_matrix();
    let result = assert_fixture("level_dependent_apex", &dense, Some(2.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    // Expected witness function: vertex 2 from level 1, vertex 4 from level 2.
    assert!(
        step.witnesses().len() >= 2,
        "edge (0, 1) needs a piecewise apex, got {:?}",
        step.witnesses()
    );
    assert_eq!(
        step.witnesses()[0],
        (1.0, 2),
        "first segment must start at the edge value with the only low candidate"
    );
    assert!(
        step.witnesses().iter().any(|&(_, apex)| apex == 4),
        "the upper level needs vertex 4 as apex, got {:?}",
        step.witnesses()
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.witnesses().len() >= 2),
        "no removal recorded more than one segment"
    );
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no apex
/// dominates it in pass 1. Vertices 4 and 5 block the two edges that would
/// otherwise dissolve the candidate 2. Edge (0, 3) leaves in pass 1, which
/// drops candidate 3 and makes (0, 1) removable in pass 2.
fn later_pass_matrix() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 1.0),
        (1, 3, 1.0),
        (0, 4, 1.0),
        (2, 4, 1.0),
        (1, 5, 1.0),
        (2, 5, 1.0),
    ];
    dense_from_edges(6, &edges)
}

#[test]
fn later_pass_removability() {
    let dense = later_pass_matrix();
    let result = assert_fixture("later_pass_removability", &dense, Some(1.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable in a later pass");
    assert_eq!(
        step.pass(),
        2,
        "edge (0, 1) must survive pass 1 and leave in pass 2"
    );
    assert!(
        result.certificate.steps().iter().any(|s| s.pass() == 2),
        "no removal happened after the first pass"
    );
    assert!(
        result.stats.passes >= 3,
        "a removal in pass 2 needs a third, empty pass, got {}",
        result.stats.passes
    );
}

#[test]
fn ties_le_vs_lt() {
    // Every comparison that decides edge (0, 1) is an equality: the candidate
    // birth b(x) equals the terminal level, the domination test f(w, x) <= t
    // holds with f(2, 3) == t, and the level is the edge's own value. A strict
    // comparison anywhere in the predicate loses this removal.
    let tied = dense_from_edges(
        4,
        &[
            (0, 1, 2.0),
            (0, 2, 2.0),
            (1, 2, 2.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
            (2, 3, 2.0),
        ],
    );
    let result = assert_fixture("ties_le_vs_lt", &tied, Some(2.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("the tied edge (0, 1) must be removable");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(2.0, 2)],
        "the tie must be certified by the first candidate at the edge value"
    );
    assert_eq!(step.pass(), 1, "the tied edge must go in pass 1");

    // Same graph without the (2, 3) tie: the two candidates are not adjacent,
    // so (0, 1) is never removable. This isolates the tie as the deciding
    // comparison above.
    let untied = dense_from_edges(
        4,
        &[
            (0, 1, 2.0),
            (0, 2, 2.0),
            (1, 2, 2.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
        ],
    );
    let result = assert_fixture("ties_le_vs_lt_untied", &untied, Some(2.0), 2, true);
    assert!(
        step_for(&result, (0, 1)).is_none(),
        "without the tie, edge (0, 1) has no dominating apex"
    );
}

#[test]
fn chordless_4cycle() {
    // No edge of a chordless cycle has a common neighbor, so nothing is
    // removable and the H1 class must survive untouched.
    let dense = dense_from_edges(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]);
    let result = assert_fixture("chordless_4cycle", &dense, Some(1.0), 2, true);
    assert!(
        result.certificate.steps().is_empty(),
        "a chordless 4-cycle has no removable edge"
    );
    assert_eq!(result.stats.passes, 1, "zero yield must take one pass");
    assert_eq!(result.matrix.num_edges(), 4, "all four edges must survive");

    let bars = dense_bars(&dense, 2, Some(1.0), 2, 1, ALL_ON, true);
    assert_eq!(essential_count(&bars, 1), 1, "the loop must stay essential");
}

#[test]
fn octahedral_sphere() {
    // The octahedron is a flag 2-sphere. Every edge has exactly two common
    // neighbors and they are antipodal, so no apex dominates and the H2 class
    // cannot be collapsed away.
    let n = 6;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let antipodal = u / 2 == v / 2;
            if !antipodal {
                edges.push((u, v, 1.0));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    let result = assert_fixture("octahedral_sphere", &dense, Some(1.0), 2, true);
    assert!(
        result.certificate.steps().is_empty(),
        "the octahedron has no removable edge"
    );
    assert_eq!(result.stats.passes, 1, "zero yield must take one pass");
    assert_eq!(
        result.matrix.num_edges(),
        12,
        "all twelve edges must survive"
    );

    for &modulus in &MODULI {
        let bars = dense_bars(&dense, 2, Some(1.0), modulus, 1, ALL_ON, true);
        assert_eq!(
            essential_count(&bars, 2),
            1,
            "the sphere class must survive at p={modulus}"
        );
        assert_eq!(essential_count(&bars, 1), 0, "no H1 at p={modulus}");
    }
}

#[test]
fn zero_distance_clusters() {
    // Duplicate points glue into clusters at distance exactly 0. Zero-value
    // edges are born at the bottom of the filtration, where the candidate set
    // is at its largest.
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.4, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        for _ in 0..3 {
            points.push(site.to_vec());
        }
    }
    let dense = DistanceMatrix::from_points(&points).unwrap();
    let result = assert_fixture("zero_distance_clusters", &dense, None, 2, true);
    for step in result.certificate.steps() {
        if step.value() == 0.0 {
            assert_eq!(
                step.witnesses()[0].0,
                0.0,
                "a zero-value edge must be certified from level 0"
            );
        }
    }
    // The engine follows ripser and drops zero-persistence pairs, so the six
    // within-cluster [0, 0) bars never appear: two finite H0 bars remain.
    let bars = dense_bars(&dense, 2, None, 2, 1, ALL_ON, true);
    assert_eq!(finite_count(&bars, 0), 2, "two cluster merges must die");
}

#[test]
fn tie_heavy_grid() {
    // L1 distances on a 4x4 grid: only integer values 1 through 6, so almost
    // every candidate shares a birth level with its neighbors and the
    // critical-value sweep runs on large tied blocks.
    let side = 4i64;
    let coords: Vec<(i64, i64)> = (0..side)
        .flat_map(|x| (0..side).map(move |y| (x, y)))
        .collect();
    let mut condensed = Vec::new();
    for i in 1..coords.len() {
        for j in 0..i {
            let d = (coords[i].0 - coords[j].0).abs() + (coords[i].1 - coords[j].1).abs();
            condensed.push(d as f64);
        }
    }
    let dense = DistanceMatrix::from_condensed(condensed).unwrap();
    assert_fixture("tie_heavy_grid", &dense, None, 1, true);
}

#[test]
fn dense_near_clique() {
    // K8 minus one edge: the candidate sets are as large as they get for this
    // size, and the missing edge is the only obstruction the predicate can
    // find.
    let n = 8;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if (u, v) != (0, 1) {
                edges.push((u, v, 1.0));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    let result = assert_fixture("dense_near_clique", &dense, Some(1.0), 2, true);
    assert!(
        !result.certificate.steps().is_empty(),
        "a near-clique must yield removals"
    );
    assert!(
        result.stats.max_common_neighborhood >= 5,
        "expected a large common neighborhood, got {}",
        result.stats.max_common_neighborhood
    );
}

#[test]
fn disconnected_plus_inf() {
    // Three components joined by nothing: the enclosing radius is +inf, so the
    // terminal level comes from the largest finite edge, and the three
    // essential H0 classes must survive the collapse.
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (3, 4, 1.0),
        (3, 5, 2.0),
        (4, 5, 2.0),
        (6, 7, 2.0),
        (6, 8, 2.0),
        (7, 8, 1.0),
    ];
    let dense = dense_from_edges(9, &edges);
    let result = assert_fixture("disconnected_plus_inf", &dense, None, 2, true);
    assert_eq!(
        result.certificate.terminal_level(),
        2.0,
        "terminal level must fall back to the largest finite edge"
    );
    for &modulus in &MODULI {
        let bars = dense_bars(&dense, 2, None, modulus, 1, ALL_ON, true);
        assert_eq!(
            essential_count(&bars, 0),
            3,
            "three components must stay separate at p={modulus}"
        );
    }
}

#[test]
fn sparse_hub() {
    // A star with a few cross edges, given directly as a sparse matrix. The
    // hub sits in every candidate set, and the leaves have degree 1 or 2.
    let n = 7;
    let mut edges: Vec<(usize, usize, f64)> = (1..n).map(|v| (0, v, 1.0)).collect();
    edges.push((1, 2, 1.0));
    edges.push((3, 4, 2.0));
    edges.push((5, 6, 1.5));
    let sparse = sparse_from_edges(n, &edges);
    let result = collapse_and_check_sparse("sparse_hub", &sparse, None);
    assert_eq!(
        result.certificate.terminal_level(),
        2.0,
        "an unthresholded sparse input ends at its largest edge"
    );
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            for threshold in [None, Some(1.5), Some(f64::INFINITY)] {
                let plain = sparse_bars(&sparse, 2, threshold, modulus, threads, ALL_ON, false);
                let collapsed = sparse_bars(&sparse, 2, threshold, modulus, threads, ALL_ON, true);
                assert_eq!(
                    plain, collapsed,
                    "sparse_hub: collapse changed the diagram \
                     (p={modulus} threads={threads} threshold={threshold:?})"
                );
            }
        }
    }
    for threshold in [None, Some(1.5)] {
        collapse_and_check_sparse("sparse_hub", &sparse, threshold);
    }
}

#[test]
fn non_metric_domination_flip() {
    // d(0,1) = 10 while d(0,2) = d(1,2) = 1: a gross triangle-inequality
    // violation. Domination is a graph property, so the long edge is still
    // dominated and must be removed. The predicate may not assume a metric.
    let dense = dense_from_edges(
        4,
        &[
            (0, 1, 10.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 5.0),
            (1, 3, 5.0),
            (2, 3, 0.5),
        ],
    );
    let result = assert_fixture("non_metric_domination_flip", &dense, Some(10.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("the long edge must be dominated");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(10.0, 2)],
        "the first candidate covers the single critical value"
    );
}

#[test]
fn projective_plane_torsion() {
    // The 13-vertex RP^2 triangulation: H1 and H2 are Z/2, visible only at
    // p = 2. The collapse must preserve the torsion answer at every modulus,
    // so it cannot be quietly field-dependent.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/projective_plane.lower_distance_matrix");
    let dense = holos_tda::io::read_lower_distance_matrix(&path).unwrap();
    collapse_and_check_dense("projective_plane", &dense, None);

    let intervals = |bars: &[Bar], dim: usize| -> Vec<(f64, f64)> {
        bars.iter()
            .filter(|b| b.dim == dim)
            .map(|b| (b.birth, b.death))
            .collect()
    };
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 2, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 2, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "collapse changed the RP^2 diagram (p={modulus} threads={threads})"
            );
            let expected = if modulus == 2 {
                vec![(1.0, 2.0)]
            } else {
                vec![]
            };
            assert_eq!(
                intervals(&collapsed, 1),
                expected,
                "collapsed H1 at p={modulus}"
            );
            assert_eq!(
                intervals(&collapsed, 2),
                expected,
                "collapsed H2 at p={modulus}"
            );
        }
    }
}

// K64,64 with every present edge at distance 1. The bipartite graph is
// triangle-free, so no edge has a candidate and the collapse must return the
// input untouched. The barcode is known: one component and
// b1 = 4096 - 128 + 1 = 3969 essential H1 classes.
const BIP_N: usize = 128;
const K4_N: usize = 4;

fn bipartite_dist(i: usize, j: usize) -> f64 {
    if (i < BIP_N / 2) != (j < BIP_N / 2) {
        1.0
    } else {
        f64::INFINITY
    }
}

fn bipartite_dense() -> DistanceMatrix {
    let mut data = Vec::with_capacity(BIP_N * (BIP_N - 1) / 2);
    for i in 1..BIP_N {
        for j in 0..i {
            data.push(bipartite_dist(i, j));
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

// The same bipartite block plus a disjoint K4. The K4 is the only place where
// a removal can happen, so it separates the two halves of the schedule.
fn bipartite_k4_dist(i: usize, j: usize) -> f64 {
    let across_bipartite = i < BIP_N && j < BIP_N && (i < BIP_N / 2) != (j < BIP_N / 2);
    let inside_k4 = i >= BIP_N && j >= BIP_N;
    if across_bipartite || inside_k4 {
        1.0
    } else {
        f64::INFINITY
    }
}

fn bipartite_k4_dense() -> DistanceMatrix {
    let n = BIP_N + K4_N;
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            data.push(bipartite_k4_dist(i, j));
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

#[test]
fn k64_64_zero_yield() {
    let dense = bipartite_dense();
    let result = collapse_and_check_dense("k64_64", &dense, None);
    assert!(
        result.certificate.steps().is_empty(),
        "a triangle-free graph has no removable edge"
    );
    assert_eq!(result.certificate.input_edge_count(), 4096, "input edges");
    assert_eq!(result.certificate.output_edge_count(), 4096, "output edges");
    assert_eq!(result.matrix.num_edges(), 4096, "surviving edges");
    assert_eq!(result.stats.passes, 1, "zero yield must take one pass");
    assert_eq!(result.stats.witness_segments, 0, "no witness segments");
    assert_eq!(
        result.stats.max_common_neighborhood, 0,
        "no edge has a common neighbor"
    );
    assert_eq!(
        result.certificate.terminal_level(),
        1.0,
        "terminal level is the only edge value"
    );

    for &modulus in &[2u32, 3] {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "K64,64: collapse changed the diagram (p={modulus} threads={threads})"
            );
            assert_eq!(
                essential_count(&collapsed, 0),
                1,
                "K64,64 is connected (p={modulus})"
            );
            assert_eq!(
                essential_count(&collapsed, 1),
                3969,
                "K64,64 cycle rank (p={modulus})"
            );
            assert_eq!(finite_count(&collapsed, 1), 0, "no finite H1 (p={modulus})");
        }
    }
}

#[test]
fn k64_64_plus_k4_mixed_yield() {
    // Only the K4 can yield: every bipartite edge is triangle-free and must
    // survive, so every removal step must sit inside the K4.
    let dense = bipartite_k4_dense();
    let result = collapse_and_check_dense("k64_64+k4", &dense, None);
    assert_eq!(
        result.certificate.input_edge_count(),
        4096 + 6,
        "input edges"
    );
    assert!(
        !result.certificate.steps().is_empty(),
        "the K4 must yield removals"
    );
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert!(
            u >= BIP_N && v >= BIP_N,
            "removal of ({u}, {v}) escaped the K4"
        );
    }
    let surviving_bipartite = result
        .matrix
        .edges()
        .filter(|&(u, v, _)| u < BIP_N && v < BIP_N)
        .count();
    assert_eq!(
        surviving_bipartite, 4096,
        "every bipartite edge must survive"
    );

    for &modulus in &[2u32, 3] {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "K64,64+K4: collapse changed the diagram (p={modulus} threads={threads})"
            );
            assert_eq!(
                essential_count(&collapsed, 0),
                2,
                "two components (p={modulus})"
            );
            assert_eq!(
                essential_count(&collapsed, 1),
                3969,
                "bipartite cycle rank (p={modulus})"
            );
        }
    }
}

// Public API surface.

#[test]
fn with_edge_collapse_sets_the_flag() {
    assert!(
        !RipsParams::new(1).collapse_edges,
        "edge collapse must be off by default"
    );
    assert!(
        !RipsParams::default().collapse_edges,
        "the default params must not collapse"
    );
    let p = RipsParams::new(2).with_edge_collapse();
    assert!(p.collapse_edges, "with_edge_collapse must set the flag");
    assert_eq!(p.max_dim, 2, "with_edge_collapse must keep max_dim");
}

#[test]
fn convenience_path_matches_the_standalone_path() {
    // The flag on RipsParams must do exactly what a caller would do by hand:
    // collapse, then run the engine on the collapsed matrix at the
    // certificate's terminal level.
    let dense = level_dependent_apex_matrix();
    let sparse = sparse_from_dense(&dense);
    for &modulus in &MODULI {
        for threshold in [None, Some(2.0), Some(f64::INFINITY)] {
            for max_dim in 0..=2 {
                let label = format!("p={modulus} threshold={threshold:?} max_dim={max_dim}");

                let convenience = dense_bars(&dense, max_dim, threshold, modulus, 1, ALL_ON, true);
                let collapsed = collapse_dense(&dense, threshold).unwrap();
                let inner = params(
                    max_dim,
                    Some(collapsed.certificate.terminal_level()),
                    modulus,
                    1,
                    ALL_ON,
                    false,
                );
                let standalone =
                    canon(&rips_persistence_sparse(&collapsed.matrix, &inner).unwrap());
                assert_eq!(convenience, standalone, "{label}: dense entry point");

                let convenience =
                    sparse_bars(&sparse, max_dim, threshold, modulus, 1, ALL_ON, true);
                let collapsed = collapse_sparse(&sparse, threshold).unwrap();
                let inner = params(
                    max_dim,
                    Some(collapsed.certificate.terminal_level()),
                    modulus,
                    1,
                    ALL_ON,
                    false,
                );
                let standalone =
                    canon(&rips_persistence_sparse(&collapsed.matrix, &inner).unwrap());
                assert_eq!(convenience, standalone, "{label}: sparse entry point");
            }
        }
    }
}

#[test]
fn sparse_edges_are_sorted_deduplicated_and_exact() {
    let triplets = [
        (4usize, 1usize, 2.5f64),
        (0, 3, 1.0),
        (2, 0, 0.5),
        (3, 4, 0.0),
        (1, 0, 1.5),
    ];
    let matrix = sparse_from_edges(5, &triplets);
    let edges = edge_list(&matrix);
    assert_eq!(
        edges,
        vec![
            (0, 1, 1.5),
            (0, 2, 0.5),
            (0, 3, 1.0),
            (1, 4, 2.5),
            (3, 4, 0.0),
        ],
        "edges() must yield each pair once, ordered, with exact values"
    );
    assert_eq!(edges.len(), matrix.num_edges(), "one item per stored edge");
    for &(u, v, d) in &edges {
        assert!(u < v, "edge ({u}, {v}) not ordered");
        assert_eq!(
            matrix.get(u, v),
            d,
            "edge ({u}, {v}) value differs from get"
        );
        assert_eq!(matrix.get(v, u), d, "edge ({u}, {v}) is not symmetric");
    }

    // The same iterator on a collapsed matrix: still sorted and unique.
    let collapsed = collapse_dense(&level_dependent_apex_matrix(), Some(2.0)).unwrap();
    let edges = edge_list(&collapsed.matrix);
    assert_eq!(
        edges.len(),
        collapsed.matrix.num_edges(),
        "collapsed matrix: one item per edge"
    );
    for pair in edges.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "collapsed matrix: edges out of order at {:?}",
            pair
        );
    }
}

// Random fuzz: production certificates on mixed dense/sparse graphs must
// pass the independent verifier, with enough removals to mean something.
#[test]
fn random_certificates_pass_the_independent_verifier() {
    let palette = [0.0, 0.5, 1.0, 1.0, 2.0, 2.5, f64::INFINITY];
    let mut rng = Rng(0xdead_beef_1234_5677);
    let mut removed_total = 0usize;
    for it in 0..300 {
        let n = 2 + rng.below(9);
        let m = n * (n - 1) / 2;
        let data: Vec<f64> = (0..m).map(|_| palette[rng.below(palette.len())]).collect();
        let dense = DistanceMatrix::from_condensed(data).unwrap();
        let threshold = match rng.below(3) {
            0 => None,
            1 => Some(1.5),
            _ => Some(f64::INFINITY),
        };
        let collapsed = collapse_dense(&dense, threshold).unwrap();
        removed_total += collapsed.stats.removed_edges;
        verify_dense(&dense, threshold, &collapsed)
            .unwrap_or_else(|e| panic!("iter {it} dense: {e}"));

        let mut triplets = Vec::new();
        for i in 1..n {
            for j in 0..i {
                let d = dense.get(i, j);
                if d.is_finite() {
                    triplets.push((i, j, d));
                }
            }
        }
        let sparse = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        let st = threshold.or(Some(f64::INFINITY));
        let collapsed_s = collapse_sparse(&sparse, st).unwrap();
        verify_sparse(&sparse, st, &collapsed_s)
            .unwrap_or_else(|e| panic!("iter {it} sparse: {e}"));
    }
    assert!(removed_total > 100, "collapse never fired: {removed_total}");
}

// The unpruned reference schedule.
//
// The production collapser skips edges whose verdict provably cannot have
// changed, with a retest-everything fallback once the affected vertex set
// grows past its marking limit. Only the test counter may move: the removal
// sequence, the pass numbers, and the certificate must equal what a collapser
// that retests every live edge every pass produces.
//
// The reference below is written from sections 1 to 4 of the specification and
// shares nothing with production: a full value matrix instead of sorted
// adjacency lists with tombstones, a candidate set rebuilt by scanning all
// vertices, an explicit critical-value list, and a full pass over every live
// edge.

struct RefStep {
    edge: (usize, usize),
    value: f64,
    pass: usize,
    witnesses: Vec<(f64, usize)>,
}

/// Everything the certificate and the output matrix record, as produced by
/// the reference schedule.
struct RefRun {
    steps: Vec<RefStep>,
    survivors: Vec<(usize, usize, f64)>,
    passes: usize,
    terminal: f64,
}

/// Section 2 predicate with the section 3 witness rule, evaluated against the
/// value matrix `f`. Returns the witness segments, or `None` when some level
/// has no dominating vertex.
fn ref_test_edge(
    f: &[Vec<f64>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
) -> Option<Vec<(f64, usize)>> {
    let mut cands: Vec<(usize, f64)> = Vec::new();
    for (x, (&du, &dv)) in f[u].iter().zip(f[v].iter()).enumerate() {
        if x == u || x == v || !du.is_finite() || !dv.is_finite() {
            continue;
        }
        let b = a.max(du).max(dv);
        if b <= terminal {
            cands.push((x, b));
        }
    }

    let mut critical: Vec<f64> = std::iter::once(a)
        .chain(cands.iter().map(|&(_, b)| b))
        .collect();
    critical.sort_by(f64::total_cmp);
    critical.dedup();

    let mut segments: Vec<(f64, usize)> = Vec::new();
    let mut apex: Option<usize> = None;
    for t in critical {
        // C_t in increasing vertex order, as the witness rule requires.
        let level: Vec<usize> = cands
            .iter()
            .filter(|&&(_, b)| b <= t)
            .map(|&(x, _)| x)
            .collect();
        if level.is_empty() {
            return None;
        }
        let dominates = |w: usize| level.iter().all(|&x| x == w || f[w][x] <= t);
        let birth = |w: usize| cands.iter().find(|&&(x, _)| x == w).map(|&(_, b)| b);
        if let Some(w) = apex {
            if birth(w).is_some_and(|b| b <= t) && dominates(w) {
                continue;
            }
        }
        let found = level.iter().copied().find(|&w| dominates(w))?;
        segments.push((t, found));
        apex = Some(found);
    }
    Some(segments)
}

/// Run the frozen schedule with no pruning: every pass tests every live edge.
/// `all_edges` is the raw edge set, `resolved` the threshold after the input's
/// own rule.
fn reference_collapse(n: usize, all_edges: &[(usize, usize, f64)], resolved: f64) -> RefRun {
    let mut edges: Vec<(usize, usize, f64)> = all_edges
        .iter()
        .copied()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.2).fold(0.0f64, f64::max)
    };
    // Decreasing value, ties by increasing combinadic index. For u < v that
    // index orders by (v, u) lexicographically.
    edges.sort_by(|a, b| b.2.total_cmp(&a.2).then((a.1, a.0).cmp(&(b.1, b.0))));

    let mut f = vec![vec![f64::INFINITY; n]; n];
    for (x, row) in f.iter_mut().enumerate() {
        row[x] = 0.0;
    }
    for &(u, v, d) in &edges {
        f[u][v] = d;
        f[v][u] = d;
    }

    let mut alive = vec![true; edges.len()];
    let mut steps: Vec<RefStep> = Vec::new();
    let mut passes = 0;
    loop {
        passes += 1;
        let mut removed_any = false;
        for i in 0..edges.len() {
            if !alive[i] {
                continue;
            }
            let (u, v, value) = edges[i];
            let Some(witnesses) = ref_test_edge(&f, u, v, value, terminal) else {
                continue;
            };
            alive[i] = false;
            f[u][v] = f64::INFINITY;
            f[v][u] = f64::INFINITY;
            steps.push(RefStep {
                edge: (u, v),
                value,
                pass: passes,
                witnesses,
            });
            removed_any = true;
        }
        if !removed_any {
            break;
        }
    }

    let mut survivors: Vec<(usize, usize, f64)> = edges
        .iter()
        .zip(&alive)
        .filter(|(_, &live)| live)
        .map(|(&e, _)| e)
        .collect();
    survivors.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    RefRun {
        steps,
        survivors,
        passes,
        terminal,
    }
}

fn reference_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> RefRun {
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let mut all = Vec::with_capacity(n * (n - 1) / 2);
    for u in 0..n {
        for v in (u + 1)..n {
            all.push((u, v, dist.get(u, v)));
        }
    }
    reference_collapse(n, &all, resolved)
}

fn reference_sparse(dist: &SparseDistanceMatrix, threshold: Option<f64>) -> RefRun {
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let all: Vec<(usize, usize, f64)> = dist.edges().collect();
    reference_collapse(dist.len(), &all, resolved)
}

/// Compare a production run against the reference, bit for bit.
fn assert_reference_match(name: &str, result: &CollapsedRips, reference: &RefRun) {
    let steps = result.certificate.steps();
    let got: Vec<(usize, usize)> = steps.iter().map(|s| s.edge()).collect();
    let want: Vec<(usize, usize)> = reference.steps.iter().map(|s| s.edge).collect();
    assert_eq!(got, want, "{name}: removal sequence");
    for (i, (got, want)) in steps.iter().zip(&reference.steps).enumerate() {
        assert_eq!(
            got.value().to_bits(),
            want.value.to_bits(),
            "{name}: step {i} value"
        );
        assert_eq!(got.pass(), want.pass, "{name}: step {i} pass number");
        assert_eq!(
            got.witnesses().len(),
            want.witnesses.len(),
            "{name}: step {i} segment count"
        );
        for (j, (a, b)) in got.witnesses().iter().zip(&want.witnesses).enumerate() {
            assert_eq!(
                a.0.to_bits(),
                b.0.to_bits(),
                "{name}: step {i} segment {j} start"
            );
            assert_eq!(a.1, b.1, "{name}: step {i} segment {j} apex");
        }
    }

    let output = edge_list(&result.matrix);
    assert_eq!(
        output.len(),
        reference.survivors.len(),
        "{name}: surviving edge count"
    );
    for (i, (a, b)) in output.iter().zip(&reference.survivors).enumerate() {
        assert_eq!((a.0, a.1), (b.0, b.1), "{name}: survivor {i} endpoints");
        assert_eq!(a.2.to_bits(), b.2.to_bits(), "{name}: survivor {i} value");
    }
    assert_eq!(result.stats.passes, reference.passes, "{name}: pass count");
    assert_eq!(
        result.certificate.terminal_level().to_bits(),
        reference.terminal.to_bits(),
        "{name}: terminal level"
    );
}

/// Largest affected vertex set seen at a removal: the common neighborhood of
/// the removed edge plus its two endpoints, in the graph as it stood before
/// that removal. Replayed here from the certificate, independently of any
/// production counter.
fn widest_removal_set(n: usize, input: &[(usize, usize, f64)], result: &CollapsedRips) -> usize {
    let mut adj = vec![vec![false; n]; n];
    for &(u, v, _) in input {
        adj[u][v] = true;
        adj[v][u] = true;
    }
    let mut widest = 0;
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        let common = (0..n)
            .filter(|&x| x != u && x != v && adj[u][x] && adj[v][x])
            .count();
        widest = widest.max(common + 2);
        adj[u][v] = false;
        adj[v][u] = false;
    }
    widest
}

#[test]
fn pruned_matches_unpruned_reference() {
    // Small tie-heavy graphs: zeros, repeated values, and absent pairs, over
    // the three threshold shapes.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    let mut rng = Rng::new(0x1eaf_c0de_0001);
    let mut removed_total = 0;
    for it in 0..300 {
        let n = 2 + rng.below(11);
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|_| palette[rng.below(palette.len())])
            .collect();
        let dense = DistanceMatrix::from_condensed(data).unwrap();
        let sparse = sparse_from_dense(&dense);
        let threshold = match rng.below(3) {
            0 => None,
            1 => Some(2.0),
            _ => Some(f64::INFINITY),
        };
        let name = format!("random {it} (n={n} threshold={threshold:?})");

        let result = collapse_dense(&dense, threshold).unwrap();
        assert_reference_match(&name, &result, &reference_dense(&dense, threshold));
        removed_total += result.stats.removed_edges;

        let result = collapse_sparse(&sparse, threshold).unwrap();
        assert_reference_match(&name, &result, &reference_sparse(&sparse, threshold));
    }
    assert!(
        removed_total > 100,
        "the sweep never collapsed anything: {removed_total}"
    );

    // A dense 76-vertex graph on two values, which takes six passes to reach
    // its fixed point. Some removal here sees a common neighborhood past the
    // marking limit, so the production collapser gives up on fine marking and
    // retests every live edge in the next pass. The reference never prunes, so
    // this is the gate on that fallback.
    let mut rng = Rng::new(0x1eaf_c0de_0002);
    let n = 76;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if rng.uniform() < 0.95 {
                edges.push((u, v, if rng.uniform() < 0.5 { 1.0 } else { 2.0 }));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    let threshold = Some(2.0);
    let result = collapse_dense(&dense, threshold).unwrap();
    assert_reference_match("dense76", &result, &reference_dense(&dense, threshold));
    assert!(
        result.stats.removed_edges > 0,
        "dense76: no removal to prune around"
    );
    let widest = widest_removal_set(n, &thresholded_dense(&dense, threshold), &result);
    assert!(
        widest > 64,
        "dense76: widest affected set is {widest}, too small to force the retest fallback"
    );

    // The mixed-yield fixture: 4,096 bipartite edges that no schedule can
    // touch, plus a K4 that collapses.
    let dense = bipartite_k4_dense();
    let result = collapse_dense(&dense, None).unwrap();
    assert_reference_match("k64_64+k4 dense", &result, &reference_dense(&dense, None));
    let sparse = sparse_from_dense(&dense);
    let result = collapse_sparse(&sparse, None).unwrap();
    assert_reference_match(
        "k64_64+k4 sparse",
        &result,
        &reference_sparse(&sparse, None),
    );
}
