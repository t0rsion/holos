use super::super::common;
use super::*;

// The unpruned reference schedule, rebuilt from sections 1 and 2 of the
// specification. It shares nothing with production: a full value matrix, a
// candidate set rebuilt by scanning every vertex, an explicit critical
// value list, and a full pass over every live edge.

pub(crate) struct RefStep {
    edge: (usize, usize),
    value: f64,
    pass: usize,
    witnesses: Vec<(f64, usize)>,
}

pub(crate) struct RefRun {
    steps: Vec<RefStep>,
    survivors: Vec<(usize, usize, f64)>,
    passes: usize,
    terminal: f64,
}

/// The predicate with the witness rule, against the value matrix `f`.
/// Returns the witness segments, or `None` when some level has no
/// dominating vertex.
/// The frozen schedule with no pruning: every pass tests every live edge.
pub(crate) fn reference_collapse(
    n: usize,
    all_edges: &[(usize, usize, f64)],
    resolved: f64,
) -> RefRun {
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
            let Some(witnesses) = common::ref_test_edge(&f, u, v, value, terminal) else {
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
        .filter(|&(_, &live)| live)
        .map(|(&e, _)| e)
        .collect();
    survivors.sort_by_key(|&(u, v, _)| (u, v));
    RefRun {
        steps,
        survivors,
        passes,
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
    reference_collapse(n, &all, resolved)
}

pub(crate) fn reference_sparse(dist: &SparseDistanceMatrix, threshold: Option<f64>) -> RefRun {
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let all: Vec<(usize, usize, f64)> = dist.edges().collect();
    reference_collapse(dist.len(), &all, resolved)
}

/// Compare an ordered run against the unpruned reference, bit for bit.
pub(crate) fn assert_matches_reference(name: &str, result: &CollapsedRips, reference: &RefRun) {
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
            want.pass,
            "{name}: step {i} pass number"
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

    let output: Vec<(usize, usize, f64)> = result.matrix.edges().collect();
    assert_eq!(
        output.len(),
        reference.survivors.len(),
        "{name}: surviving edge count"
    );
    for (i, (a, b)) in output.iter().zip(&reference.survivors).enumerate() {
        assert_eq!((a.0, a.1), (b.0, b.1), "{name}: survivor {i} endpoints");
        assert_eq!(a.2.to_bits(), b.2.to_bits(), "{name}: survivor {i} value");
    }
    assert_eq!(result.stats.epochs, reference.passes, "{name}: pass count");
    assert_eq!(
        result.certificate.terminal_level().to_bits(),
        reference.terminal.to_bits(),
        "{name}: terminal level"
    );
}

/// One dense input against both references: the shipped serial run and the
/// unpruned reference.
/// `window` of `None` takes the production window through the public
/// entry point; `Some(w)` forces a window so stages cross the input.
pub(crate) fn assert_trace_dense(
    name: &str,
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) -> usize {
    let ordered = match window {
        None => collapse_dense_ordered_parallel(dist, threshold, threads).unwrap(),
        Some(w) => collapse_dense_ordered_with_window(dist, threshold, threads, w).unwrap(),
    };
    let serial = collapse_dense(dist, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_dense(dist, threshold));
    verify_dense(dist, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the ordered certificate: {e}"));
    ordered.stats.removed_edges
}

pub(crate) fn assert_trace_sparse(
    name: &str,
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) {
    let ordered = match window {
        None => collapse_sparse_ordered_parallel(dist, threshold, threads).unwrap(),
        Some(w) => collapse_sparse_ordered_with_window(dist, threshold, threads, w).unwrap(),
    };
    let serial = collapse_sparse(dist, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_sparse(dist, threshold));
    verify_sparse(dist, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the ordered certificate: {e}"));
}
