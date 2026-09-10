use super::common;
use super::fixtures::*;

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
    assert_eq!(result.stats.epochs, reference.passes, "{name}: pass count");
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
