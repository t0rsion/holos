//! Independent checker for [`CollapseCertificate`](super::CollapseCertificate).
//!
//! The checker re-derives the thresholded input and replays every
//! recorded removal against the spec text. It shares no sweep,
//! scheduling, or witness-selection code with the collapser; it is slow
//! by design and exists to catch a wrong collapse, not to be fast.
//!
//! A passing certificate establishes:
//!
//! - Header consistency: vertex count, requested threshold, terminal
//!   level, and input and output edge counts all match the input, the
//!   output matrix, and each other.
//! - Replay safety: each step removes a live edge with the recorded
//!   value, and at every critical value of the current graph the active
//!   witness apex satisfies the domination inequalities. Every witness
//!   segment starts at an independently recomputed critical value at or
//!   below the terminal level, so every segment is the active segment at
//!   its own start and no segment escapes the apex check.
//! - Witness-rule fidelity: the segments are exactly what the frozen
//!   selection rule produces. A kept apex still dominates. A new segment
//!   opens only where the previous apex stopped dominating, and its apex
//!   is the first dominating vertex of the candidate set in increasing
//!   vertex order.
//! - Schedule order: the first step is in pass 1, pass numbers never
//!   decrease and never skip a pass, and steps inside one pass follow the
//!   frozen edge order: value descending, ties by ascending combinadic
//!   index of the endpoint pair.
//! - Output and fixed point: after the last step the live edges equal the
//!   output matrix bit for bit, and no live edge is still removable.
//!
//! The checker does not certify schedule completeness. Every recorded
//! step is checked, but nothing proves the schedule visited every edge:
//! a certificate may skip a removable edge, remove it later than the
//! frozen schedule would, or leave it out entirely as long as the final
//! graph is a fixed point. Removing a different safe subset in a
//! different order therefore verifies. Only a full re-run establishes
//! the canonical production trace; the test suite does that by comparing
//! production certificates against an unpruned reference collapser.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use super::CollapsedRips;
use crate::{DistanceMatrix, SparseDistanceMatrix};

/// A failed certificate check: which step failed (when one did) and what
/// rule it broke.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyError {
    /// 0-based index of the failing removal step; `None` for header,
    /// output, or fixed-point failures.
    pub step: Option<usize>,
    /// What was violated.
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.step {
            Some(i) => write!(f, "certificate step {i}: {}", self.message),
            None => write!(f, "certificate: {}", self.message),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Check a certificate against the dense input it claims to describe.
///
/// `threshold` must be the value passed to the collapse. On success the
/// certificate replays cleanly, the replayed graph equals
/// `result.matrix`, and no further edge is removable.
pub fn verify_dense(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    result: &CollapsedRips,
) -> Result<(), VerifyError> {
    let n = dist.len();
    let resolved = resolve_threshold(threshold, || dist.enclosing_radius())?;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let d = dist.get(u, v);
            if d.is_finite() && d <= resolved {
                edges.push((u, v, d));
            }
        }
    }
    verify_common(n, threshold, resolved, &edges, result)
}

/// Check a certificate against the sparse input it claims to describe.
///
/// Same contract as [`verify_dense`].
pub fn verify_sparse(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    result: &CollapsedRips,
) -> Result<(), VerifyError> {
    let n = dist.len();
    let resolved = resolve_threshold(threshold, || f64::INFINITY)?;
    let edges: Vec<(usize, usize, f64)> = dist.edges().filter(|&(_, _, d)| d <= resolved).collect();
    verify_common(n, threshold, resolved, &edges, result)
}

fn fail(step: Option<usize>, message: impl Into<String>) -> VerifyError {
    VerifyError {
        step,
        message: message.into(),
    }
}

fn resolve_threshold(
    threshold: Option<f64>,
    default: impl FnOnce() -> f64,
) -> Result<f64, VerifyError> {
    match threshold {
        None => Ok(default()),
        Some(t) if t.is_nan() => Err(fail(None, "requested threshold is NaN")),
        Some(t) if t < 0.0 => Err(fail(None, format!("requested threshold {t} is negative"))),
        Some(t) => Ok(t),
    }
}

/// Current edge value: 0 on the diagonal, +inf when the pair is absent.
fn edge_value(adj: &[BTreeMap<usize, f64>], i: usize, j: usize) -> f64 {
    if i == j {
        return 0.0;
    }
    adj[i].get(&j).copied().unwrap_or(f64::INFINITY)
}

/// Candidate set `C` for the edge `{u, v}` with value `a` in the current
/// graph: common neighbors `x` with `b(x) = max(a, f(u,x), f(v,x))` at or
/// below the terminal level, as `(x, b(x))` in increasing vertex order.
fn candidates(
    adj: &[BTreeMap<usize, f64>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for (&x, &fu) in &adj[u] {
        if x == v {
            continue;
        }
        let fv = edge_value(adj, v, x);
        if fv.is_finite() {
            let b = a.max(fu).max(fv);
            if b <= terminal {
                out.push((x, b));
            }
        }
    }
    out
}

/// Sorted distinct values of `{a} union { b(x) : x in C }`.
fn critical_values(a: f64, cands: &[(usize, f64)]) -> Vec<f64> {
    let mut vals: Vec<f64> = std::iter::once(a)
        .chain(cands.iter().map(|&(_, b)| b))
        .collect();
    vals.sort_unstable_by(f64::total_cmp);
    vals.dedup_by(|x, y| x.to_bits() == y.to_bits());
    vals
}

/// True when `w` dominates the edge `{u, v}` at level `t`: `w` is a
/// common neighbor, enters at or below `t`, and reaches every member of
/// `C_t` at or below `t`.
fn vertex_dominates(
    adj: &[BTreeMap<usize, f64>],
    u: usize,
    v: usize,
    a: f64,
    cands: &[(usize, f64)],
    w: usize,
    t: f64,
) -> bool {
    if w == u || w == v {
        return false;
    }
    let fu = edge_value(adj, u, w);
    let fv = edge_value(adj, v, w);
    if !fu.is_finite() || !fv.is_finite() || a.max(fu).max(fv) > t {
        return false;
    }
    cands
        .iter()
        .filter(|&&(_, b)| b <= t)
        .all(|&(x, _)| edge_value(adj, w, x) <= t)
}

/// The section 2 predicate, evaluated directly: true when at every
/// critical value some candidate dominates the edge.
fn edge_removable(adj: &[BTreeMap<usize, f64>], u: usize, v: usize, a: f64, terminal: f64) -> bool {
    let cands = candidates(adj, u, v, a, terminal);
    critical_values(a, &cands).iter().all(|&t| {
        cands
            .iter()
            .any(|&(w, b)| b <= t && vertex_dominates(adj, u, v, a, &cands, w, t))
    })
}

fn verify_common(
    n: usize,
    requested: Option<f64>,
    resolved: f64,
    input_edges: &[(usize, usize, f64)],
    result: &CollapsedRips,
) -> Result<(), VerifyError> {
    let cert = &result.certificate;

    if cert.algorithm_version() != 1 {
        return Err(fail(
            None,
            format!(
                "unsupported algorithm version {} (expected 1)",
                cert.algorithm_version()
            ),
        ));
    }
    if cert.vertex_count() != n {
        return Err(fail(
            None,
            format!(
                "vertex count mismatch: certificate records {}, input has {n}",
                cert.vertex_count()
            ),
        ));
    }
    if result.matrix.len() != n {
        return Err(fail(
            None,
            format!(
                "output matrix has {} vertices, input has {n}",
                result.matrix.len()
            ),
        ));
    }
    let threshold_matches = match (cert.requested_threshold(), requested) {
        (None, None) => true,
        (Some(a), Some(b)) => a.to_bits() == b.to_bits(),
        _ => false,
    };
    if !threshold_matches {
        return Err(fail(
            None,
            format!(
                "requested threshold mismatch: certificate records {:?}, caller passed {requested:?}",
                cert.requested_threshold()
            ),
        ));
    }
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        input_edges.iter().map(|&(_, _, d)| d).fold(0.0, f64::max)
    };
    if cert.terminal_level().to_bits() != terminal.to_bits() {
        return Err(fail(
            None,
            format!(
                "terminal level mismatch: certificate records {}, input gives {terminal}",
                cert.terminal_level()
            ),
        ));
    }
    if cert.input_edge_count() != input_edges.len() {
        return Err(fail(
            None,
            format!(
                "input edge count mismatch: certificate records {}, thresholded input has {}",
                cert.input_edge_count(),
                input_edges.len()
            ),
        ));
    }
    if cert.output_edge_count() != result.matrix.num_edges() {
        return Err(fail(
            None,
            format!(
                "output edge count mismatch: certificate records {}, output matrix has {}",
                cert.output_edge_count(),
                result.matrix.num_edges()
            ),
        ));
    }
    if cert.input_edge_count() != cert.output_edge_count() + cert.steps().len() {
        return Err(fail(
            None,
            format!(
                "edge count invariant violated: input {} != output {} + {} steps",
                cert.input_edge_count(),
                cert.output_edge_count(),
                cert.steps().len()
            ),
        ));
    }

    let mut adj: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for &(u, v, d) in input_edges {
        adj[u].insert(v, d);
        adj[v].insert(u, d);
    }

    let mut prev_pass = 1usize;
    let mut prev_key: Option<(f64, (usize, usize))> = None;
    for (k, step) in cert.steps().iter().enumerate() {
        if step.pass() == 0 {
            return Err(fail(
                Some(k),
                "pass number 0 is invalid; passes are 1-based",
            ));
        }
        if k == 0 {
            if step.pass() != 1 {
                return Err(fail(
                    Some(k),
                    format!("first step is in pass {}, expected pass 1", step.pass()),
                ));
            }
        } else if step.pass() < prev_pass {
            return Err(fail(
                Some(k),
                format!("pass number {} decreases from {prev_pass}", step.pass()),
            ));
        } else if step.pass() > prev_pass + 1 {
            return Err(fail(
                Some(k),
                format!("pass number {} skips pass {}", step.pass(), prev_pass + 1),
            ));
        }
        if step.pass() != prev_pass {
            prev_key = None;
        }
        prev_pass = step.pass();

        let (u, v) = step.edge();
        if u >= v || v >= n {
            return Err(fail(
                Some(k),
                format!("edge ({u}, {v}) is not a valid vertex pair for n = {n}"),
            ));
        }
        let a = step.value();
        // (v, u) orders exactly like the combinadic index v*(v-1)/2 + u for
        // u < v, and the field compare cannot overflow.
        let lex = (v, u);
        if let Some((prev_value, prev_index)) = prev_key {
            let out_of_order = match a.total_cmp(&prev_value) {
                Ordering::Greater => true,
                Ordering::Equal => lex <= prev_index,
                Ordering::Less => false,
            };
            if out_of_order {
                return Err(fail(
                    Some(k),
                    format!(
                        "edge ({u}, {v}) with value {a} breaks the schedule order within pass {}",
                        step.pass()
                    ),
                ));
            }
        }
        prev_key = Some((a, lex));
        match adj[u].get(&v) {
            None => {
                return Err(fail(
                    Some(k),
                    format!("edge ({u}, {v}) is not live at this step"),
                ));
            }
            Some(&live) if live.to_bits() != a.to_bits() => {
                return Err(fail(
                    Some(k),
                    format!("edge ({u}, {v}) value mismatch: step records {a}, graph has {live}"),
                ));
            }
            Some(_) => {}
        }

        let segments = step.witnesses();
        let Some(&(first_start, _)) = segments.first() else {
            return Err(fail(Some(k), "step has no witness segments"));
        };
        if first_start.to_bits() != a.to_bits() {
            return Err(fail(
                Some(k),
                format!("first witness segment starts at {first_start}, edge value is {a}"),
            ));
        }
        for &(s, w) in segments {
            if s.is_nan() {
                return Err(fail(Some(k), "witness segment start is NaN"));
            }
            if s > terminal {
                return Err(fail(
                    Some(k),
                    format!("witness segment starts at {s}, after terminal level {terminal}"),
                ));
            }
            if w >= n {
                return Err(fail(
                    Some(k),
                    format!("witness apex {w} is not a vertex index for n = {n}"),
                ));
            }
        }
        for pair in segments.windows(2) {
            if pair[1].0 <= pair[0].0 {
                return Err(fail(
                    Some(k),
                    format!(
                        "witness segment starts are not strictly increasing: {} then {}",
                        pair[0].0, pair[1].0
                    ),
                ));
            }
        }

        let cands = candidates(&adj, u, v, a, terminal);
        let crits = critical_values(a, &cands);
        if segments.len() > crits.len() {
            return Err(fail(
                Some(k),
                format!(
                    "{} witness segments exceed {} critical values",
                    segments.len(),
                    crits.len()
                ),
            ));
        }
        for &(s, _) in segments {
            if !crits.iter().any(|&c| c.to_bits() == s.to_bits()) {
                return Err(fail(
                    Some(k),
                    format!(
                        "witness segment start {s} is not a critical value of the current graph"
                    ),
                ));
            }
        }
        for &t in &crits {
            let mut active = None;
            for (i, &(s, w)) in segments.iter().enumerate() {
                if s <= t {
                    active = Some((i, s, w));
                } else {
                    break;
                }
            }
            let Some((seg_index, seg_start, w)) = active else {
                return Err(fail(
                    Some(k),
                    format!("no witness segment is active at critical value {t}"),
                ));
            };
            if w == u || w == v {
                return Err(fail(
                    Some(k),
                    format!("apex {w} is an endpoint of edge ({u}, {v})"),
                ));
            }
            let fu = edge_value(&adj, u, w);
            let fv = edge_value(&adj, v, w);
            if !fu.is_finite() || !fv.is_finite() {
                return Err(fail(
                    Some(k),
                    format!("apex {w} is not a common neighbor of {u} and {v}"),
                ));
            }
            let bw = a.max(fu).max(fv);
            if bw > t {
                return Err(fail(
                    Some(k),
                    format!("apex {w} enters at {bw}, after critical value {t}"),
                ));
            }
            for &(x, bx) in &cands {
                if bx <= t {
                    let fwx = edge_value(&adj, w, x);
                    if fwx > t {
                        return Err(fail(
                            Some(k),
                            format!(
                                "apex {w} does not dominate at critical value {t}: f({w}, {x}) = {fwx}"
                            ),
                        ));
                    }
                }
            }
            if seg_start.to_bits() == t.to_bits() {
                if seg_index > 0 {
                    let prev_w = segments[seg_index - 1].1;
                    if vertex_dominates(&adj, u, v, a, &cands, prev_w, t) {
                        return Err(fail(
                            Some(k),
                            format!(
                                "segment starting at {t} is redundant: previous apex {prev_w} still dominates"
                            ),
                        ));
                    }
                }
                for &(x, bx) in &cands {
                    if x >= w {
                        break;
                    }
                    if bx <= t && vertex_dominates(&adj, u, v, a, &cands, x, t) {
                        return Err(fail(
                            Some(k),
                            format!(
                                "apex {w} is not the first dominating vertex at {t}: vertex {x} also dominates"
                            ),
                        ));
                    }
                }
            }
        }

        adj[u].remove(&v);
        adj[v].remove(&u);
    }

    let mut live: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for (u, list) in adj.iter().enumerate() {
        for (&v, &d) in list {
            if u < v {
                live.insert((u, v), d);
            }
        }
    }
    for (u, v, d) in result.matrix.edges() {
        match live.remove(&(u, v)) {
            None => {
                return Err(fail(
                    None,
                    format!("output matrix has extra edge ({u}, {v})"),
                ));
            }
            Some(r) if r.to_bits() != d.to_bits() => {
                return Err(fail(
                    None,
                    format!(
                        "output edge ({u}, {v}) value mismatch: replay keeps {r}, matrix has {d}"
                    ),
                ));
            }
            Some(_) => {}
        }
    }
    if let Some((&(u, v), _)) = live.iter().next() {
        return Err(fail(
            None,
            format!("output matrix is missing edge ({u}, {v})"),
        ));
    }

    for (u, list) in adj.iter().enumerate() {
        for (&v, &d) in list {
            if u < v && edge_removable(&adj, u, v, d, terminal) {
                return Err(fail(
                    None,
                    format!("fixed point violated: edge ({u}, {v}) is still removable"),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collapse::{CollapseCertificate, CollapseStats, RemovalStep};

    fn triangle_dense() -> DistanceMatrix {
        DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap()
    }

    fn triangle_sparse() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0)]).unwrap()
    }

    fn stats_for(input: usize, output: usize) -> CollapseStats {
        CollapseStats {
            input_edges: input,
            output_edges: output,
            removed_edges: input - output,
            passes: 1,
            edge_tests: 0,
            witness_segments: 0,
            max_common_neighborhood: 0,
        }
    }

    /// Hand-built result for the unit triangle: edge (0, 1) removed with
    /// the single witness segment (1.0, apex 2); the path 0-2, 1-2 remains.
    fn triangle_result() -> CollapsedRips {
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(3, &[(0, 2, 1.0), (1, 2, 1.0)]).unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 3,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 3,
                output_edge_count: 2,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    pass: 1,
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(3, 2),
        }
    }

    /// Same removal as [`triangle_result`], but at requested threshold 2.0
    /// so the terminal level sits above the only critical value 1.0.
    fn triangle_result_threshold_two() -> CollapsedRips {
        let mut result = triangle_result();
        result.certificate.requested_threshold = Some(2.0);
        result.certificate.terminal_level = 2.0;
        result
    }

    fn k4_dense() -> DistanceMatrix {
        DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
    }

    /// Hand-built result for the unit K4: pass 1 removes (0, 1), (0, 2),
    /// and (1, 2) in schedule order; the star at vertex 3 remains.
    fn k4_result() -> CollapsedRips {
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(
                4,
                &[(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)],
            )
            .unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 4,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 6,
                output_edge_count: 3,
                steps: vec![
                    RemovalStep {
                        u: 0,
                        v: 1,
                        value: 1.0,
                        pass: 1,
                        witnesses: vec![(1.0, 2)],
                    },
                    RemovalStep {
                        u: 0,
                        v: 2,
                        value: 1.0,
                        pass: 1,
                        witnesses: vec![(1.0, 3)],
                    },
                    RemovalStep {
                        u: 1,
                        v: 2,
                        value: 1.0,
                        pass: 1,
                        witnesses: vec![(1.0, 3)],
                    },
                ],
            },
            stats: stats_for(6, 3),
        }
    }

    /// Result with a two-level candidate set for edge (0, 1): vertex 2
    /// enters at 1.0, vertex 3 at 2.0, and f(2, 3) = 2.0 keeps vertex 2
    /// dominating through the terminal level 2.0. The single recorded step
    /// is valid, but the graph is not a fixed point afterward; use this
    /// fixture only for rejections that fire inside step 0.
    fn two_level_dense() -> DistanceMatrix {
        DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0]).unwrap()
    }

    fn two_level_result() -> CollapsedRips {
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(
                4,
                &[
                    (0, 2, 1.0),
                    (0, 3, 2.0),
                    (1, 2, 1.0),
                    (1, 3, 2.0),
                    (2, 3, 2.0),
                ],
            )
            .unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 4,
                requested_threshold: Some(2.0),
                terminal_level: 2.0,
                input_edge_count: 6,
                output_edge_count: 5,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    pass: 1,
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(6, 5),
        }
    }

    #[test]
    fn accepts_triangle_removal_dense() {
        assert_eq!(
            verify_dense(&triangle_dense(), None, &triangle_result()),
            Ok(())
        );
    }

    #[test]
    fn accepts_triangle_removal_sparse() {
        assert_eq!(
            verify_sparse(&triangle_sparse(), None, &triangle_result()),
            Ok(())
        );
    }

    #[test]
    fn rejects_wrong_edge_value() {
        let mut result = triangle_result();
        result.certificate.steps[0].value = 2.0;
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("value mismatch"), "{}", err.message);
    }

    #[test]
    fn rejects_endpoint_witness_apex() {
        let mut result = triangle_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 1)];
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("apex"), "{}", err.message);
    }

    #[test]
    fn rejects_non_dominating_apex() {
        // Two apex candidates 2 and 3 that are not neighbors of each other:
        // neither dominates, so no removal of (0, 1) can verify.
        let dist =
            DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0, 1.0, 1.0, f64::INFINITY]).unwrap();
        let result = CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(
                4,
                &[(0, 2, 1.0), (1, 2, 1.0), (0, 3, 1.0), (1, 3, 1.0)],
            )
            .unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 4,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 5,
                output_edge_count: 4,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    pass: 1,
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(5, 4),
        };
        let err = verify_dense(&dist, None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("dominate"), "{}", err.message);
    }

    #[test]
    fn rejects_segment_start_after_edge_value() {
        let mut result = triangle_result();
        result.certificate.steps[0].witnesses = vec![(1.5, 2)];
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("starts at"), "{}", err.message);
    }

    #[test]
    fn rejects_missing_step() {
        let mut result = triangle_result();
        result.certificate.steps.clear();
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(err.message.contains("invariant"), "{}", err.message);
    }

    #[test]
    fn rejects_altered_output_edge() {
        let mut result = triangle_result();
        result.matrix =
            SparseDistanceMatrix::from_triplets(3, &[(0, 2, 1.5), (1, 2, 1.0)]).unwrap();
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(err.message.contains("value mismatch"), "{}", err.message);
    }

    #[test]
    fn rejects_extra_output_edge() {
        let mut result = triangle_result();
        result.matrix = triangle_sparse();
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(err.message.contains("output edge count"), "{}", err.message);
    }

    #[test]
    fn rejects_missed_removable_edge() {
        // Empty certificate on the full triangle: every edge is removable,
        // so the fixed-point scan must object.
        let result = CollapsedRips {
            matrix: triangle_sparse(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 3,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 3,
                output_edge_count: 3,
                steps: vec![],
            },
            stats: stats_for(3, 3),
        };
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(err.message.contains("removable"), "{}", err.message);
    }

    #[test]
    fn accepts_isolated_edge_with_empty_certificate() {
        // A single edge has no common neighbor, so it is not removable and
        // the empty certificate is the correct fixed point.
        let dist = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
        let result = CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(2, &[(0, 1, 1.0)]).unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                vertex_count: 2,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 1,
                output_edge_count: 1,
                steps: vec![],
            },
            stats: stats_for(1, 1),
        };
        assert_eq!(verify_dense(&dist, None, &result), Ok(()));
    }

    #[test]
    fn rejects_zero_pass_number() {
        let mut result = triangle_result();
        result.certificate.steps[0].pass = 0;
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("1-based"), "{}", err.message);
    }

    #[test]
    fn rejects_equal_segment_starts() {
        let mut result = triangle_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.0, 2)];
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(
            err.message.contains("strictly increasing"),
            "{}",
            err.message
        );
    }

    #[test]
    fn rejects_wrong_terminal_level() {
        let mut result = triangle_result();
        result.certificate.terminal_level = 2.0;
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, None);
        assert!(err.message.contains("terminal level"), "{}", err.message);
    }

    #[test]
    fn rejects_requested_threshold_mismatch() {
        let err = verify_dense(&triangle_dense(), Some(1.0), &triangle_result()).unwrap_err();
        assert_eq!(err.step, None);
        assert!(
            err.message.contains("requested threshold"),
            "{}",
            err.message
        );
    }

    #[test]
    fn accepts_triangle_removal_below_threshold() {
        assert_eq!(
            verify_dense(
                &triangle_dense(),
                Some(2.0),
                &triangle_result_threshold_two()
            ),
            Ok(())
        );
    }

    #[test]
    fn rejects_forged_appended_segment() {
        // The reviewed forgery: at threshold 2 the only critical value is
        // 1.0, so the appended segment (1.5, 0) was never the active
        // segment and slipped through unchecked.
        let mut result = triangle_result_threshold_two();
        result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 0)];
        let err = verify_dense(&triangle_dense(), Some(2.0), &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("critical value"), "{}", err.message);
    }

    #[test]
    fn rejects_segment_start_after_terminal() {
        let mut result = triangle_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 2)];
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("terminal level"), "{}", err.message);
    }

    #[test]
    fn rejects_segment_at_non_critical_value() {
        // Two segments fit under the two critical values 1.0 and 2.0, but
        // 1.5 is not one of them.
        let mut result = two_level_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 2), (1.5, 3)];
        let err = verify_dense(&two_level_dense(), Some(2.0), &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(
            err.message.contains("not a critical value"),
            "{}",
            err.message
        );
    }

    #[test]
    fn rejects_redundant_segment() {
        // Apex 2 still dominates at 2.0, so the frozen rule keeps it and
        // never opens the recorded segment (2.0, 3).
        let mut result = two_level_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 2), (2.0, 3)];
        let err = verify_dense(&two_level_dense(), Some(2.0), &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("redundant"), "{}", err.message);
    }

    #[test]
    fn accepts_k4_collapse() {
        assert_eq!(verify_dense(&k4_dense(), None, &k4_result()), Ok(()));
    }

    #[test]
    fn rejects_apex_not_first_in_vertex_order() {
        // Vertices 2 and 3 both dominate (0, 1) at 1.0; the frozen rule
        // takes 2, so recording 3 is a different selection.
        let mut result = k4_result();
        result.certificate.steps[0].witnesses = vec![(1.0, 3)];
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("first dominating"), "{}", err.message);
    }

    #[test]
    fn rejects_out_of_schedule_order_within_pass() {
        // Swapping (0, 2) and (1, 2) still replays cleanly, but pass 1
        // must visit combinadic index 1 before index 2.
        let mut result = k4_result();
        result.certificate.steps.swap(1, 2);
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(2));
        assert!(err.message.contains("schedule order"), "{}", err.message);
    }

    #[test]
    fn rejects_first_pass_not_one() {
        let mut result = triangle_result();
        result.certificate.steps[0].pass = 2;
        let err = verify_dense(&triangle_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("first step"), "{}", err.message);
    }

    #[test]
    fn accepts_production_certificates() {
        // The verifier certifies the frozen rules exactly, so nothing the
        // production collapser emits may fail. Sweep small graphs with
        // ties, zeros, and +inf entries under several thresholds.
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let values = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
        for n in 3..8usize {
            for round in 0..8u64 {
                let m = n * (n - 1) / 2;
                let cond: Vec<f64> = (0..m)
                    .map(|_| values[(next() % values.len() as u64) as usize])
                    .collect();
                let dense = DistanceMatrix::from_condensed(cond).unwrap();
                let threshold = match round % 3 {
                    0 => None,
                    1 => Some(2.0),
                    _ => Some(f64::INFINITY),
                };
                let r = crate::collapse::collapse_dense(&dense, threshold).unwrap();
                verify_dense(&dense, threshold, &r).unwrap();

                let triplets: Vec<(usize, usize, f64)> = (0..n)
                    .flat_map(|u| (u + 1..n).map(move |v| (u, v)))
                    .map(|(u, v)| (u, v, dense.get(u, v)))
                    .filter(|&(_, _, d)| d.is_finite())
                    .collect();
                let sparse = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
                let r = crate::collapse::collapse_sparse(&sparse, threshold).unwrap();
                verify_sparse(&sparse, threshold, &r).unwrap();
            }
        }
    }

    #[test]
    fn rejects_pass_gap() {
        let mut result = k4_result();
        result.certificate.steps[1].pass = 3;
        result.certificate.steps[2].pass = 3;
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("skips pass 2"), "{}", err.message);
    }
}
