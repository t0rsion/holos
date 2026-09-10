use super::super::common;
use super::support::edge_list;
use holos_tda::collapse::CollapsedRips;
use holos_tda::{DistanceMatrix, SparseDistanceMatrix};

// The unpruned version 2 reference schedule.
//
// Production may skip an edge whose verdict provably cannot have changed
// since its last test, and falls back to retesting everything once the
// affected vertex set grows past the marking limit. Only the test counter
// may move: the removal sequence, the round numbers, the witnesses, and the
// surviving graph must equal what a collapser that retests every live edge
// every round produces.
//
// The reference below is written from sections 1 and 2 of the version 2
// specification and the unchanged predicate of the version 1 specification.
// It shares nothing with production: a full value matrix instead of sorted
// adjacency lists with tombstones, a candidate set rebuilt by scanning all
// vertices, an explicit critical-value list, a cloned snapshot per round, a
// full test of every live edge, and a conflict check against every earlier
// selection of the round.

pub(crate) struct RefStep {
    edge: (usize, usize),
    value: f64,
    epoch: usize,
    witnesses: Vec<(f64, usize)>,
}

/// One live edge that passed the predicate against the round's snapshot.
pub(crate) struct RefSuccess {
    u: usize,
    v: usize,
    value: f64,
    witnesses: Vec<(f64, usize)>,
}

/// Everything the certificate and the output matrix record, as produced by
/// the reference schedule.
pub(crate) struct RefRun {
    steps: Vec<RefStep>,
    survivors: Vec<(usize, usize, f64)>,
    epochs: usize,
    terminal: f64,
}

pub(crate) fn reference_matrix(n: usize, edges: &[(usize, usize, f64)]) -> Vec<Vec<f64>> {
    let mut matrix = vec![vec![f64::INFINITY; n]; n];
    for (vertex, row) in matrix.iter_mut().enumerate() {
        row[vertex] = 0.0;
    }
    for &(u, v, value) in edges {
        matrix[u][v] = value;
        matrix[v][u] = value;
    }
    matrix
}

pub(crate) fn reference_successes(
    snapshot: &[Vec<f64>],
    live: &[(usize, usize, f64)],
    terminal: f64,
) -> Vec<RefSuccess> {
    let mut successes: Vec<_> = live
        .iter()
        .filter_map(|&(u, v, value)| {
            common::ref_test_edge(snapshot, u, v, value, terminal).map(|witnesses| RefSuccess {
                u,
                v,
                value,
                witnesses,
            })
        })
        .collect();
    successes.sort_by(|a, b| {
        b.value
            .total_cmp(&a.value)
            .then((a.v, a.u).cmp(&(b.v, b.u)))
    });
    successes
}

pub(crate) fn reference_read_set(snapshot: &[Vec<f64>], u: usize, v: usize) -> Vec<bool> {
    (0..snapshot.len())
        .map(|vertex| {
            vertex == u
                || vertex == v
                || (snapshot[u][vertex].is_finite() && snapshot[v][vertex].is_finite())
        })
        .collect()
}

pub(crate) fn reference_batch<'a>(
    snapshot: &[Vec<f64>],
    successes: &'a [RefSuccess],
) -> Vec<&'a RefSuccess> {
    let mut read_sets: Vec<Vec<bool>> = Vec::new();
    let mut batch = Vec::new();
    for success in successes {
        if read_sets.iter().any(|set| set[success.u] && set[success.v]) {
            continue;
        }
        read_sets.push(reference_read_set(snapshot, success.u, success.v));
        batch.push(success);
    }
    batch
}

pub(crate) fn retire_reference_batch(
    matrix: &mut [Vec<f64>],
    steps: &mut Vec<RefStep>,
    batch: &[&RefSuccess],
    epoch: usize,
) {
    for success in batch {
        steps.push(RefStep {
            edge: (success.u, success.v),
            value: success.value,
            epoch,
            witnesses: success.witnesses.clone(),
        });
        matrix[success.u][success.v] = f64::INFINITY;
        matrix[success.v][success.u] = f64::INFINITY;
    }
}

/// The version 1 section 2 predicate with the section 3 witness rule,
/// evaluated against the value matrix `f`. Returns the witness segments, or
/// `None` when some level has no dominating vertex. The version 2 schedule
/// leaves this rule untouched; only the graph it reads changes.
/// Run the version 2 schedule with no pruning: every round tests every live
/// edge against a frozen snapshot, sorts the successes by value descending
/// with ties by ascending (v, u), then takes them greedily while no earlier
/// selection of the round has both endpoints of the candidate in its closed
/// common neighborhood.
pub(crate) fn reference_collapse_v2(
    n: usize,
    all_edges: &[(usize, usize, f64)],
    resolved: f64,
) -> RefRun {
    let edges: Vec<(usize, usize, f64)> = all_edges
        .iter()
        .copied()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.2).fold(0.0f64, f64::max)
    };

    let mut f = reference_matrix(n, &edges);

    let mut live = edges.clone();
    let mut steps: Vec<RefStep> = Vec::new();
    let mut epochs = 0;
    loop {
        epochs += 1;
        let snapshot = f.clone();

        let successes = reference_successes(&snapshot, &live, terminal);
        if successes.is_empty() {
            break;
        }
        let batch = reference_batch(&snapshot, &successes);
        retire_reference_batch(&mut f, &mut steps, &batch, epochs);
        live.retain(|&(u, v, _)| f[u][v].is_finite());
    }

    let mut survivors = live;
    survivors.sort_by_key(|&(u, v, _)| (u, v));
    RefRun {
        steps,
        survivors,
        epochs,
        terminal,
    }
}

pub(crate) fn reference_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> RefRun {
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let mut all = Vec::with_capacity(n * (n - 1) / 2);
    for u in 0..n {
        for v in (u + 1)..n {
            all.push((u, v, dist.get(u, v)));
        }
    }
    reference_collapse_v2(n, &all, resolved)
}

pub(crate) fn reference_sparse(dist: &SparseDistanceMatrix, threshold: Option<f64>) -> RefRun {
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let all: Vec<(usize, usize, f64)> = dist.edges().collect();
    reference_collapse_v2(dist.len(), &all, resolved)
}

/// Compare a production run against the reference, bit for bit.
pub(crate) fn assert_reference_match(name: &str, result: &CollapsedRips, reference: &RefRun) {
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
        assert_eq!(
            got.position().number(),
            want.epoch,
            "{name}: step {i} round number"
        );
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
    assert_eq!(result.stats.epochs, reference.epochs, "{name}: round count");
    assert_eq!(
        result.certificate.terminal_level().to_bits(),
        reference.terminal.to_bits(),
        "{name}: terminal level"
    );
}
