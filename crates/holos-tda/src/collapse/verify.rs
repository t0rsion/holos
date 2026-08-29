//! Independent checker for [`CollapseCertificate`].
//!
//! The checker re-derives the thresholded input and replays every
//! recorded removal. It shares no sweep, scheduling, or witness-selection
//! code with the collapser. It is slow by design: its job is to catch a
//! wrong collapse.
//!
//! The checker dispatches on the certificate's algorithm version.
//! Version 1 replays the serial schedule: each step is checked against
//! the graph left by the steps before it. Version 2 replays the rounds
//! schedule: steps are grouped by round, every check in a round runs
//! against the graph as it stood before the round, and the round's edges
//! are deleted only after the whole round passes. Version 3 replays an
//! unstructured adaptive sequence. It checks the safety of every removal.
//! It does not reproduce or certify the ranking policy.
//!
//! A passing certificate establishes:
//!
//! - Header consistency: vertex count, requested threshold, terminal
//!   level, and input and output edge counts all match the input, the
//!   output matrix, and each other.
//! - Replay safety: each step removes a live edge with the recorded
//!   value. At every critical value of the reference graph the active
//!   witness apex satisfies the domination inequalities. The reference
//!   graph is the current replay state for version 1 and the pre-round
//!   graph for version 2. Every witness segment starts at an independently
//!   recomputed critical value at or below the terminal level. Every
//!   segment is therefore the active segment at its own start, and no
//!   segment escapes the apex check.
//! - Witness-rule fidelity: the segments are exactly what the frozen
//!   selection rule produces on the reference graph. A kept apex still
//!   dominates. A new segment opens only where the previous apex stopped
//!   dominating, and its apex is the first dominating vertex of the
//!   candidate set in increasing vertex order.
//! - Schedule order: passes, rounds, and sequence positions are 1-based.
//!   Pass and round numbers never decrease or skip. Steps inside a pass or
//!   round follow the frozen edge order: value descending, ties by ascending
//!   combinadic index of the endpoint pair.
//! - Round independence, version 2 only: for every ordered pair of steps
//!   in one round, the closed common neighborhood of the first edge,
//!   taken in the pre-round graph, does not contain both endpoints of the
//!   second. A round that groups conflicting removals is rejected even
//!   when replaying its steps one after the other would succeed.
//! - Output and fixed point: after the last step the live edges equal the
//!   output matrix bit for bit. A certificate marked `CompleteFixedPoint`
//!   also proves that no live edge is still removable. A certificate marked
//!   `BudgetLimited` makes no fixed-point claim.
//!
//! The checker does not certify that a run followed the production
//! scheduling policy. A complete certificate proves a fixed-point result.
//! It may reach that result through a different safe trace. Version 2
//! does not require each round to be a greedy-maximal batch. Version 3 does
//! not check that the highest-scoring removal came first. Only a full
//! production re-run establishes the canonical production trace.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use super::wire::{CollapseArtifact, graph_digest};
use super::{
    CollapseCertificate, CollapseCompleteness, CollapseObjective, CollapsedRips, RemovalStep,
    SchedulePosition,
};
use crate::{DistanceMatrix, SparseDistanceMatrix};

/// A failed certificate check: which step failed, when one did, and what
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
/// certificate replays cleanly and the replayed graph equals
/// `result.matrix`.
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
    verify_common(
        n,
        threshold,
        resolved,
        &edges,
        &result.matrix,
        &result.certificate,
    )
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
    let edges: Vec<(usize, usize, f64)> = dist
        .edges()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    verify_common(
        n,
        threshold,
        resolved,
        &edges,
        &result.matrix,
        &result.certificate,
    )
}

/// Check an artifact against the dense input it claims to describe.
///
/// Checks the artifact's cryptographic graph bindings first, then runs the
/// same replay as [`verify_dense`].
pub fn verify_dense_artifact(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    artifact: &CollapseArtifact,
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
    check_bindings(n, &edges, artifact)?;
    verify_common(
        n,
        threshold,
        resolved,
        &edges,
        artifact.matrix(),
        artifact.certificate(),
    )
}

/// Check an artifact against the sparse input it claims to describe.
///
/// Checks the artifact's cryptographic graph bindings first, then runs the
/// same replay as [`verify_sparse`].
pub fn verify_sparse_artifact(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    artifact: &CollapseArtifact,
) -> Result<(), VerifyError> {
    let n = dist.len();
    let resolved = resolve_threshold(threshold, || f64::INFINITY)?;
    let edges: Vec<_> = dist
        .edges()
        .filter(|&(_, _, d)| d.is_finite() && d <= resolved)
        .collect();
    check_bindings(n, &edges, artifact)?;
    verify_common(
        n,
        threshold,
        resolved,
        &edges,
        artifact.matrix(),
        artifact.certificate(),
    )
}

fn check_bindings(
    n: usize,
    input_edges: &[(usize, usize, f64)],
    artifact: &CollapseArtifact,
) -> Result<(), VerifyError> {
    if graph_digest(n, input_edges) != artifact.input_digest() {
        return Err(fail(
            None,
            "input graph does not match the artifact binding",
        ));
    }
    let output: Vec<_> = artifact.matrix().edges().collect();
    if graph_digest(artifact.matrix().len(), &output) != artifact.output_digest() {
        return Err(fail(
            None,
            "output graph does not match the artifact binding",
        ));
    }
    Ok(())
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

/// Check that the step names a valid vertex pair; return the endpoints and
/// the recorded value.
fn check_edge_pair(
    n: usize,
    k: usize,
    step: &RemovalStep,
) -> Result<(usize, usize, f64), VerifyError> {
    let (u, v) = step.edge();
    if u >= v || v >= n {
        return Err(fail(
            Some(k),
            format!("edge ({u}, {v}) is not a valid vertex pair for n = {n}"),
        ));
    }
    Ok((u, v, step.value()))
}

/// Check that the edge is live in `adj` with the recorded value, bit for
/// bit.
fn check_live_value(
    adj: &[BTreeMap<usize, f64>],
    k: usize,
    u: usize,
    v: usize,
    a: f64,
) -> Result<(), VerifyError> {
    match adj[u].get(&v) {
        None => Err(fail(
            Some(k),
            format!("edge ({u}, {v}) is not live at this step"),
        )),
        Some(&live) if live.to_bits() != a.to_bits() => Err(fail(
            Some(k),
            format!("edge ({u}, {v}) value mismatch: step records {a}, graph has {live}"),
        )),
        Some(_) => Ok(()),
    }
}

/// True when `(a, lex)` breaks the frozen order after `prev`: value
/// descending, ties by ascending `(v, u)`.
fn breaks_schedule_order(prev: Option<(f64, (usize, usize))>, a: f64, lex: (usize, usize)) -> bool {
    match prev {
        None => false,
        Some((prev_value, prev_index)) => match a.total_cmp(&prev_value) {
            Ordering::Greater => true,
            Ordering::Equal => lex <= prev_index,
            Ordering::Less => false,
        },
    }
}

/// Validate the step's witness segments against `adj` with the full frozen
/// selection rule.
fn check_witnesses(
    adj: &[BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    k: usize,
    edge: (usize, usize, f64),
    segments: &[(f64, usize)],
) -> Result<(), VerifyError> {
    let (u, v, a) = edge;
    check_witness_header(k, a, segments)?;
    check_witness_metadata(n, terminal, k, segments)?;
    check_witness_order(k, segments)?;
    let cands = candidates(adj, u, v, a, terminal);
    let crits = critical_values(a, &cands);
    check_witness_starts(k, segments, &crits)?;
    for &critical in &crits {
        check_witness_at_level(adj, k, edge, &cands, segments, critical)?;
    }
    Ok(())
}

fn check_witness_header(
    step: usize,
    edge_value: f64,
    segments: &[(f64, usize)],
) -> Result<(), VerifyError> {
    let Some(&(first_start, _)) = segments.first() else {
        return Err(fail(Some(step), "step has no witness segments"));
    };
    if first_start.to_bits() != edge_value.to_bits() {
        return Err(fail(
            Some(step),
            format!("first witness segment starts at {first_start}, edge value is {edge_value}"),
        ));
    }
    Ok(())
}

fn check_witness_metadata(
    n: usize,
    terminal: f64,
    step: usize,
    segments: &[(f64, usize)],
) -> Result<(), VerifyError> {
    for &(s, w) in segments {
        if s.is_nan() {
            return Err(fail(Some(step), "witness segment start is NaN"));
        }
        if s > terminal {
            return Err(fail(
                Some(step),
                format!("witness segment starts at {s}, after terminal level {terminal}"),
            ));
        }
        if w >= n {
            return Err(fail(
                Some(step),
                format!("witness apex {w} is not a vertex index for n = {n}"),
            ));
        }
    }
    Ok(())
}

fn check_witness_order(step: usize, segments: &[(f64, usize)]) -> Result<(), VerifyError> {
    for pair in segments.windows(2) {
        if pair[1].0 <= pair[0].0 {
            return Err(fail(
                Some(step),
                format!(
                    "witness segment starts are not strictly increasing: {} then {}",
                    pair[0].0, pair[1].0
                ),
            ));
        }
    }
    Ok(())
}

fn check_witness_starts(
    step: usize,
    segments: &[(f64, usize)],
    critical_values: &[f64],
) -> Result<(), VerifyError> {
    if segments.len() > critical_values.len() {
        return Err(fail(
            Some(step),
            format!(
                "{} witness segments exceed {} critical values",
                segments.len(),
                critical_values.len()
            ),
        ));
    }
    for &(s, _) in segments {
        if !critical_values
            .iter()
            .any(|&critical| critical.to_bits() == s.to_bits())
        {
            return Err(fail(
                Some(step),
                format!("witness segment start {s} is not a critical value of the current graph"),
            ));
        }
    }
    Ok(())
}

fn active_witness(segments: &[(f64, usize)], critical: f64) -> Option<(usize, f64, usize)> {
    segments
        .iter()
        .enumerate()
        .take_while(|entry| entry.1.0 <= critical)
        .last()
        .map(|(index, &(start, apex))| (index, start, apex))
}

fn check_witness_at_level(
    adj: &[BTreeMap<usize, f64>],
    step: usize,
    edge: (usize, usize, f64),
    candidates: &[(usize, f64)],
    segments: &[(f64, usize)],
    critical: f64,
) -> Result<(), VerifyError> {
    let Some((segment_index, segment_start, apex)) = active_witness(segments, critical) else {
        return Err(fail(
            Some(step),
            format!("no witness segment is active at critical value {critical}"),
        ));
    };
    check_active_apex(adj, step, edge, candidates, apex, critical)?;
    if segment_start.to_bits() == critical.to_bits() {
        check_segment_selection(
            adj,
            step,
            edge,
            candidates,
            segments,
            segment_index,
            apex,
            critical,
        )?;
    }
    Ok(())
}

fn check_active_apex(
    adj: &[BTreeMap<usize, f64>],
    step: usize,
    edge: (usize, usize, f64),
    candidates: &[(usize, f64)],
    apex: usize,
    critical: f64,
) -> Result<(), VerifyError> {
    let (u, v, value) = edge;
    if apex == u || apex == v {
        return Err(fail(
            Some(step),
            format!("apex {apex} is an endpoint of edge ({u}, {v})"),
        ));
    }
    let fu = edge_value(adj, u, apex);
    let fv = edge_value(adj, v, apex);
    if !fu.is_finite() || !fv.is_finite() {
        return Err(fail(
            Some(step),
            format!("apex {apex} is not a common neighbor of {u} and {v}"),
        ));
    }
    let birth = value.max(fu).max(fv);
    if birth > critical {
        return Err(fail(
            Some(step),
            format!("apex {apex} enters at {birth}, after critical value {critical}"),
        ));
    }
    check_apex_domination(adj, step, candidates, apex, critical)
}

fn check_apex_domination(
    adj: &[BTreeMap<usize, f64>],
    step: usize,
    candidates: &[(usize, f64)],
    apex: usize,
    critical: f64,
) -> Result<(), VerifyError> {
    for &(vertex, birth) in candidates {
        let distance = edge_value(adj, apex, vertex);
        if birth <= critical && distance > critical {
            return Err(fail(
                Some(step),
                format!(
                    "apex {apex} does not dominate at critical value {critical}: f({apex}, {vertex}) = {distance}"
                ),
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn check_segment_selection(
    adj: &[BTreeMap<usize, f64>],
    step: usize,
    edge: (usize, usize, f64),
    candidates: &[(usize, f64)],
    segments: &[(f64, usize)],
    segment_index: usize,
    apex: usize,
    critical: f64,
) -> Result<(), VerifyError> {
    let (u, v, value) = edge;
    if segment_index > 0 {
        let previous_apex = segments[segment_index - 1].1;
        if vertex_dominates(adj, u, v, value, candidates, previous_apex, critical) {
            return Err(fail(
                Some(step),
                format!(
                    "segment starting at {critical} is redundant: previous apex {previous_apex} still dominates"
                ),
            ));
        }
    }
    for &(vertex, birth) in candidates.iter().take_while(|&&(vertex, _)| vertex < apex) {
        if birth <= critical && vertex_dominates(adj, u, v, value, candidates, vertex, critical) {
            return Err(fail(
                Some(step),
                format!(
                    "apex {apex} is not the first dominating vertex at {critical}: vertex {vertex} also dominates"
                ),
            ));
        }
    }
    Ok(())
}

/// Replay a version 1 certificate: serial passes, each step checked
/// against the graph left by the steps before it.
fn replay_serial(
    adj: &mut [BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    let mut prev_pass = 1usize;
    let mut prev_key: Option<(f64, (usize, usize))> = None;
    for (k, step) in cert.steps().iter().enumerate() {
        let pass = serial_pass(k, step, prev_pass)?;
        if pass != prev_pass {
            prev_key = None;
        }
        prev_pass = pass;
        let (u, v, value) = check_ordered_step(adj, n, k, step, pass, prev_key, "pass")?;
        prev_key = Some((value, (v, u)));
        check_witnesses(adj, n, terminal, k, (u, v, value), step.witnesses())?;
        adj[u].remove(&v);
        adj[v].remove(&u);
    }
    Ok(())
}

fn serial_pass(k: usize, step: &RemovalStep, previous: usize) -> Result<usize, VerifyError> {
    let SchedulePosition::Pass(pass) = step.position() else {
        return Err(fail(Some(k), "version 1 step is not positioned in a pass"));
    };
    if pass == 0 {
        return Err(fail(
            Some(k),
            "pass number 0 is invalid; passes are 1-based",
        ));
    }
    if k == 0 && pass != 1 {
        return Err(fail(
            Some(k),
            format!("first step is in pass {pass}, expected pass 1"),
        ));
    }
    if pass < previous {
        return Err(fail(
            Some(k),
            format!("pass number {pass} decreases from {previous}"),
        ));
    }
    if pass > previous + 1 {
        return Err(fail(
            Some(k),
            format!("pass number {pass} skips pass {}", previous + 1),
        ));
    }
    Ok(pass)
}

fn check_ordered_step(
    adj: &[BTreeMap<usize, f64>],
    n: usize,
    k: usize,
    step: &RemovalStep,
    group: usize,
    previous: Option<(f64, (usize, usize))>,
    group_name: &str,
) -> Result<(usize, usize, f64), VerifyError> {
    let (u, v, value) = check_edge_pair(n, k, step)?;
    // `(v, u)` has combinadic order without computing a potentially large index.
    if breaks_schedule_order(previous, value, (v, u)) {
        return Err(fail(
            Some(k),
            format!(
                "edge ({u}, {v}) with value {value} breaks the schedule order within {group_name} {group}"
            ),
        ));
    }
    check_live_value(adj, k, u, v, value)?;
    Ok((u, v, value))
}

/// Replay a version 3 certificate in its unstructured removal order.
///
/// The position is a sequence number. It does not encode a pass, round, or
/// priority-policy claim.
fn replay_adaptive(
    adj: &mut [BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    for (k, step) in cert.steps().iter().enumerate() {
        let expected = k + 1;
        let SchedulePosition::Sequence(sequence) = step.position() else {
            return Err(fail(
                Some(k),
                "version 3 step is not positioned in an adaptive sequence",
            ));
        };
        if sequence != expected {
            return Err(fail(
                Some(k),
                format!("adaptive sequence position is {sequence}, expected {expected}"),
            ));
        }
        let (u, v, a) = check_edge_pair(n, k, step)?;
        check_live_value(adj, k, u, v, a)?;
        check_witnesses(adj, n, terminal, k, (u, v, a), step.witnesses())?;
        adj[u].remove(&v);
        adj[v].remove(&u);
    }
    Ok(())
}

/// True when `x` lies in the closed common neighborhood of the live edge
/// `{u, v}`: `x` is an endpoint or is adjacent to both endpoints.
fn in_closed_common(adj: &[BTreeMap<usize, f64>], u: usize, v: usize, x: usize) -> bool {
    let near_u = x == u || adj[u].contains_key(&x);
    let near_v = x == v || adj[v].contains_key(&x);
    near_u && near_v
}

/// Replay a version 2 certificate. Each round is checked in full against
/// the pre-round graph. Deletions apply only after the whole round passes.
fn replay_rounds(
    adj: &mut [BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    let steps = cert.steps();
    for (start, end) in collect_rounds(steps)? {
        replay_round(adj, n, terminal, steps, start, end)?;
    }
    Ok(())
}

fn collect_rounds(steps: &[RemovalStep]) -> Result<Vec<(usize, usize)>, VerifyError> {
    let mut rounds: Vec<(usize, usize)> = Vec::new();
    for (k, step) in steps.iter().enumerate() {
        let SchedulePosition::Round(round) = step.position() else {
            return Err(fail(Some(k), "version 2 step is not positioned in a round"));
        };
        if round == 0 {
            return Err(fail(
                Some(k),
                "round number 0 is invalid; rounds are 1-based",
            ));
        }
        extend_round_ranges(steps, k, round, &mut rounds)?;
    }
    Ok(rounds)
}

fn extend_round_ranges(
    steps: &[RemovalStep],
    k: usize,
    round: usize,
    ranges: &mut Vec<(usize, usize)>,
) -> Result<(), VerifyError> {
    let Some(last) = ranges.last_mut() else {
        if round != 1 {
            return Err(fail(
                Some(k),
                format!("first step is in round {round}, expected round 1"),
            ));
        }
        ranges.push((k, k + 1));
        return Ok(());
    };
    let SchedulePosition::Round(previous) = steps[last.1 - 1].position() else {
        unreachable!("round positions were checked while collecting ranges")
    };
    if round < previous {
        return Err(fail(
            Some(k),
            format!("round number {round} decreases from {previous}"),
        ));
    }
    if round > previous + 1 {
        return Err(fail(
            Some(k),
            format!("round number {round} skips round {}", previous + 1),
        ));
    }
    if round == previous {
        last.1 = k + 1;
    } else {
        ranges.push((k, k + 1));
    }
    Ok(())
}

fn replay_round(
    adj: &mut [BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    steps: &[RemovalStep],
    start: usize,
    end: usize,
) -> Result<(), VerifyError> {
    let SchedulePosition::Round(round) = steps[start].position() else {
        unreachable!("round positions were checked while collecting ranges")
    };
    let round_steps = &steps[start..end];
    check_round_order(adj, n, round, start, round_steps)?;
    check_round_independence(adj, round, start, round_steps)?;
    check_round_witnesses(adj, n, terminal, start, round_steps)?;
    remove_round(adj, round_steps);
    Ok(())
}

fn check_round_order(
    adj: &[BTreeMap<usize, f64>],
    n: usize,
    round: usize,
    start: usize,
    steps: &[RemovalStep],
) -> Result<(), VerifyError> {
    let mut previous = None;
    for (offset, step) in steps.iter().enumerate() {
        let (_, v, value) =
            check_ordered_step(adj, n, start + offset, step, round, previous, "round")?;
        previous = Some((value, (v, step.edge().0)));
    }
    Ok(())
}

fn check_round_independence(
    adj: &[BTreeMap<usize, f64>],
    round: usize,
    start: usize,
    steps: &[RemovalStep],
) -> Result<(), VerifyError> {
    let in_round: BTreeMap<(usize, usize), usize> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| (step.edge(), index))
        .collect();
    for (index, step) in steps.iter().enumerate() {
        if let Some(conflict) = round_conflict(adj, step.edge(), index, &in_round) {
            let (u, v) = step.edge();
            let (conflict_u, conflict_v) = steps[conflict].edge();
            return Err(fail(
                Some(start + conflict),
                format!(
                    "round {round} groups conflicting removals: both endpoints of ({conflict_u}, {conflict_v}) lie in the closed common neighborhood of ({u}, {v})"
                ),
            ));
        }
    }
    Ok(())
}

fn round_conflict(
    adj: &[BTreeMap<usize, f64>],
    edge: (usize, usize),
    edge_index: usize,
    in_round: &BTreeMap<(usize, usize), usize>,
) -> Option<usize> {
    let (u, v) = edge;
    let mut closed: Vec<usize> = adj[u]
        .keys()
        .filter(|vertex| adj[v].contains_key(vertex))
        .copied()
        .collect();
    closed.push(u);
    closed.push(v);
    closed.sort_unstable();
    debug_assert!(closed.iter().all(|&x| in_closed_common(adj, u, v, x)));
    let mut conflict = None;
    for (position, &x) in closed.iter().enumerate() {
        for &y in &closed[position + 1..] {
            if let Some(&other) = in_round.get(&(x, y))
                && other != edge_index
                && conflict.is_none_or(|current| other < current)
            {
                conflict = Some(other);
            }
        }
    }
    conflict
}

fn check_round_witnesses(
    adj: &[BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    start: usize,
    steps: &[RemovalStep],
) -> Result<(), VerifyError> {
    for (offset, step) in steps.iter().enumerate() {
        let (u, v) = step.edge();
        check_witnesses(
            adj,
            n,
            terminal,
            start + offset,
            (u, v, step.value()),
            step.witnesses(),
        )?;
    }
    Ok(())
}

fn remove_round(adj: &mut [BTreeMap<usize, f64>], steps: &[RemovalStep]) {
    for step in steps {
        let (u, v) = step.edge();
        adj[u].remove(&v);
        adj[v].remove(&u);
    }
}

fn verify_common(
    n: usize,
    requested: Option<f64>,
    resolved: f64,
    input_edges: &[(usize, usize, f64)],
    matrix: &SparseDistanceMatrix,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    let version = check_version_metadata(cert)?;
    check_header(n, requested, input_edges, matrix, cert)?;
    let terminal = terminal_level(resolved, input_edges);
    if cert.terminal_level().to_bits() != terminal.to_bits() {
        return Err(fail(
            None,
            format!(
                "terminal level mismatch: certificate records {}, input gives {terminal}",
                cert.terminal_level()
            ),
        ));
    }
    let mut adjacency = input_adjacency(n, input_edges);
    replay_version(version, &mut adjacency, n, terminal, cert)?;
    check_output(&adjacency, matrix)?;
    check_fixed_point(&adjacency, terminal, cert)
}

fn check_version_metadata(cert: &CollapseCertificate) -> Result<u32, VerifyError> {
    let version = cert.algorithm_version();
    if !(1..=3).contains(&version) {
        return Err(fail(
            None,
            format!("unsupported algorithm version {version} (expected 1, 2, or 3)"),
        ));
    }
    match version {
        1 | 2 => check_fixed_schedule_metadata(version, cert)?,
        3 => check_adaptive_metadata(cert)?,
        _ => unreachable!(),
    }
    Ok(version)
}

fn check_fixed_schedule_metadata(
    version: u32,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    if cert.objective().is_some()
        || cert.completeness() != CollapseCompleteness::CompleteFixedPoint
        || cert.work_limit().is_some()
        || cert.work_used() != 0
    {
        return Err(fail(
            None,
            format!("algorithm version {version} carries version 3 schedule metadata"),
        ));
    }
    Ok(())
}

fn check_adaptive_metadata(cert: &CollapseCertificate) -> Result<(), VerifyError> {
    if !matches!(
        cert.objective(),
        Some(CollapseObjective::H1 | CollapseObjective::H2)
    ) {
        return Err(fail(None, "algorithm version 3 has no collapse objective"));
    }
    if let Some(limit) = cert.work_limit()
        && cert.work_used() > limit
    {
        return Err(fail(
            None,
            format!(
                "adaptive work used {} exceeds its limit {limit}",
                cert.work_used()
            ),
        ));
    }
    if cert.completeness() == CollapseCompleteness::BudgetLimited {
        check_budget_limit(cert)?;
    }
    Ok(())
}

fn check_budget_limit(cert: &CollapseCertificate) -> Result<(), VerifyError> {
    let Some(limit) = cert.work_limit() else {
        return Err(fail(None, "budget-limited certificate has no work limit"));
    };
    if cert.work_used() != limit {
        return Err(fail(
            None,
            format!(
                "budget-limited certificate used {} work units, expected its limit {limit}",
                cert.work_used()
            ),
        ));
    }
    Ok(())
}

fn check_header(
    n: usize,
    requested: Option<f64>,
    input_edges: &[(usize, usize, f64)],
    matrix: &SparseDistanceMatrix,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    if cert.vertex_count() != n {
        return Err(fail(
            None,
            format!(
                "vertex count mismatch: certificate records {}, input has {n}",
                cert.vertex_count()
            ),
        ));
    }
    if matrix.len() != n {
        return Err(fail(
            None,
            format!("output matrix has {} vertices, input has {n}", matrix.len()),
        ));
    }
    check_requested_threshold(requested, cert)?;
    check_edge_counts(input_edges.len(), matrix.num_edges(), cert)
}

fn check_requested_threshold(
    requested: Option<f64>,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
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
    Ok(())
}

fn terminal_level(resolved: f64, input_edges: &[(usize, usize, f64)]) -> f64 {
    if resolved.is_finite() {
        resolved
    } else {
        input_edges
            .iter()
            .map(|&(_, _, distance)| distance)
            .fold(0.0, f64::max)
    }
}

fn check_edge_counts(
    input_edges: usize,
    output_edges: usize,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    if cert.input_edge_count() != input_edges {
        return Err(fail(
            None,
            format!(
                "input edge count mismatch: certificate records {}, thresholded input has {}",
                cert.input_edge_count(),
                input_edges
            ),
        ));
    }
    if cert.output_edge_count() != output_edges {
        return Err(fail(
            None,
            format!(
                "output edge count mismatch: certificate records {}, output matrix has {}",
                cert.output_edge_count(),
                output_edges
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
    Ok(())
}

fn input_adjacency(n: usize, input_edges: &[(usize, usize, f64)]) -> Vec<BTreeMap<usize, f64>> {
    let mut adj: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for &(u, v, d) in input_edges {
        adj[u].insert(v, d);
        adj[v].insert(u, d);
    }
    adj
}

fn replay_version(
    version: u32,
    adjacency: &mut [BTreeMap<usize, f64>],
    n: usize,
    terminal: f64,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    match version {
        1 => replay_serial(adjacency, n, terminal, cert),
        2 => replay_rounds(adjacency, n, terminal, cert),
        3 => replay_adaptive(adjacency, n, terminal, cert),
        _ => unreachable!(),
    }
}

fn check_output(
    adjacency: &[BTreeMap<usize, f64>],
    matrix: &SparseDistanceMatrix,
) -> Result<(), VerifyError> {
    let mut live: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for (u, list) in adjacency.iter().enumerate() {
        for (&v, &d) in list {
            if u < v {
                live.insert((u, v), d);
            }
        }
    }
    for (u, v, d) in matrix.edges() {
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
    Ok(())
}

fn check_fixed_point(
    adjacency: &[BTreeMap<usize, f64>],
    terminal: f64,
    cert: &CollapseCertificate,
) -> Result<(), VerifyError> {
    if cert.completeness() != CollapseCompleteness::CompleteFixedPoint {
        return Ok(());
    }
    for (u, list) in adjacency.iter().enumerate() {
        for (&v, &distance) in list {
            if u < v && edge_removable(adjacency, u, v, distance, terminal) {
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
    use crate::collapse::{
        AdaptiveCollapseParams, CollapseCertificate, CollapseCompleteness, CollapseObjective,
        CollapseStats, RemovalStep, collapse_dense_adaptive, collapse_sparse_rounds_parallel,
    };

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
            epochs: 1,
            edge_tests: 0,
            witness_segments: 0,
            max_common_neighborhood: 0,
            window_slots_offered: 0,
            window_members_formed: 0,
            window_members_reused: 0,
            logical_tests: 0,
            invalidated_results: 0,
            global_invalidations: 0,
            window_batches: 0,
            adaptive_score_evaluations: 0,
            adaptive_queue_pops: 0,
            adaptive_stale_pops: 0,
            adaptive_triangles_removed: 0,
            adaptive_tetrahedra_removed: 0,
        }
    }

    /// Hand-built result for the unit triangle: edge (0, 1) removed with
    /// the single witness segment (1.0, apex 2); the path 0-2, 1-2 remains.
    fn triangle_result() -> CollapsedRips {
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(3, &[(0, 2, 1.0), (1, 2, 1.0)]).unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 1,
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 3,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 3,
                output_edge_count: 2,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(3, 2),
            timings: Default::default(),
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
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
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
                        position: SchedulePosition::Pass(1),
                        witnesses: vec![(1.0, 2)],
                    },
                    RemovalStep {
                        u: 0,
                        v: 2,
                        value: 1.0,
                        position: SchedulePosition::Pass(1),
                        witnesses: vec![(1.0, 3)],
                    },
                    RemovalStep {
                        u: 1,
                        v: 2,
                        value: 1.0,
                        position: SchedulePosition::Pass(1),
                        witnesses: vec![(1.0, 3)],
                    },
                ],
            },
            stats: stats_for(6, 3),
            timings: Default::default(),
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
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 4,
                requested_threshold: Some(2.0),
                terminal_level: 2.0,
                input_edge_count: 6,
                output_edge_count: 5,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(6, 5),
            timings: Default::default(),
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
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 4,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 5,
                output_edge_count: 4,
                steps: vec![RemovalStep {
                    u: 0,
                    v: 1,
                    value: 1.0,
                    position: SchedulePosition::Pass(1),
                    witnesses: vec![(1.0, 2)],
                }],
            },
            stats: stats_for(5, 4),
            timings: Default::default(),
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
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 3,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 3,
                output_edge_count: 3,
                steps: vec![],
            },
            stats: stats_for(3, 3),
            timings: Default::default(),
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
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 2,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 1,
                output_edge_count: 1,
                steps: vec![],
            },
            stats: stats_for(1, 1),
            timings: Default::default(),
        };
        assert_eq!(verify_dense(&dist, None, &result), Ok(()));
    }

    #[test]
    fn rejects_zero_pass_number() {
        let mut result = triangle_result();
        result.certificate.steps[0].position = SchedulePosition::Pass(0);
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
        result.certificate.steps[0].position = SchedulePosition::Pass(2);
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
        result.certificate.steps[1].position = SchedulePosition::Pass(3);
        result.certificate.steps[2].position = SchedulePosition::Pass(3);
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("skips pass 2"), "{}", err.message);
    }

    /// Two disjoint unit K4s on vertices 0-3 and 4-7; cross edges are
    /// absent, so the enclosing radius is infinite and the terminal level
    /// is the largest finite edge value 1.0.
    fn two_k4_dense() -> DistanceMatrix {
        let mut cond = Vec::new();
        for v in 1..8usize {
            for u in 0..v {
                cond.push(if (u < 4) == (v < 4) {
                    1.0
                } else {
                    f64::INFINITY
                });
            }
        }
        DistanceMatrix::from_condensed(cond).unwrap()
    }

    fn v2_step(u: usize, v: usize, round: usize, apex: usize) -> RemovalStep {
        RemovalStep {
            u,
            v,
            value: 1.0,
            position: SchedulePosition::Round(round),
            witnesses: vec![(1.0, apex)],
        }
    }

    /// Hand-derived version 2 result for the two disjoint K4s. Round 1
    /// removes (0, 1) and (4, 5); each blocks every other edge of its own
    /// component. Round 2 removes (0, 2), (1, 2), (4, 6), (5, 6): in each
    /// round 2 snapshot the surviving hub (3 or 7) is the only candidate,
    /// and the two selected edges do not conflict. The stars at vertices
    /// 3 and 7 remain and no further edge is removable.
    fn two_k4_v2_result() -> CollapsedRips {
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(
                8,
                &[
                    (0, 3, 1.0),
                    (1, 3, 1.0),
                    (2, 3, 1.0),
                    (4, 7, 1.0),
                    (5, 7, 1.0),
                    (6, 7, 1.0),
                ],
            )
            .unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 2,
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 8,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 12,
                output_edge_count: 6,
                steps: vec![
                    v2_step(0, 1, 1, 2),
                    v2_step(4, 5, 1, 6),
                    v2_step(0, 2, 2, 3),
                    v2_step(1, 2, 2, 3),
                    v2_step(4, 6, 2, 7),
                    v2_step(5, 6, 2, 7),
                ],
            },
            stats: stats_for(12, 6),
            timings: Default::default(),
        }
    }

    /// Version 2 result for the unit K4 with one recorded round; callers
    /// pick the steps. The output matrix is the K4 minus the removed
    /// edges, so the header checks pass and the replay reaches the round.
    fn k4_v2_round(steps: Vec<RemovalStep>) -> CollapsedRips {
        let gone: Vec<(usize, usize)> = steps.iter().map(|s| (s.u, s.v)).collect();
        let survivors: Vec<(usize, usize, f64)> = (0..4usize)
            .flat_map(|u| (u + 1..4).map(move |v| (u, v, 1.0)))
            .filter(|&(u, v, _)| !gone.contains(&(u, v)))
            .collect();
        CollapsedRips {
            matrix: SparseDistanceMatrix::from_triplets(4, &survivors).unwrap(),
            certificate: CollapseCertificate {
                algorithm_version: 2,
                objective: None,
                completeness: CollapseCompleteness::CompleteFixedPoint,
                work_limit: None,
                work_used: 0,
                vertex_count: 4,
                requested_threshold: None,
                terminal_level: 1.0,
                input_edge_count: 6,
                output_edge_count: survivors.len(),
                steps,
            },
            stats: stats_for(6, survivors.len()),
            timings: Default::default(),
        }
    }

    #[test]
    fn accepts_two_k4_v2_rounds() {
        assert_eq!(
            verify_dense(&two_k4_dense(), None, &two_k4_v2_result()),
            Ok(())
        );
    }

    #[test]
    fn rejects_same_round_conflict_despite_serial_validity() {
        // The serial-safe forgery: (0, 1) apex 2 then (0, 2) apex 3
        // replay cleanly one after the other, but both endpoints of
        // (0, 2) lie in S((0, 1)) = {0, 1, 2, 3}, so one round cannot
        // hold both.
        let result = k4_v2_round(vec![v2_step(0, 1, 1, 2), v2_step(0, 2, 1, 3)]);
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("conflict"), "{}", err.message);
    }

    #[test]
    fn rejects_witnesses_valid_only_after_earlier_step() {
        // Step 1 records apex 3 for (1, 2). The frozen rule selects 3
        // only after (0, 1) is gone; against the round snapshot the apex
        // is vertex 0, so a verifier that mutates between steps would
        // accept this pair. Any such in-round dependence puts both
        // endpoints of (1, 2) inside S((0, 1)), so the nonconflict check
        // rejects the grouping.
        let result = k4_v2_round(vec![v2_step(0, 1, 1, 2), v2_step(1, 2, 1, 3)]);
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("conflict"), "{}", err.message);
    }

    #[test]
    fn rejects_witnesses_from_stale_snapshot() {
        // Round 2 records apex 1 for (0, 2). Vertex 1 was the frozen
        // choice in the round 1 graph, but round 1 deleted (0, 1), so in
        // the round 2 snapshot vertex 1 is no longer a common neighbor.
        let mut result = two_k4_v2_result();
        result.certificate.steps[2].witnesses = vec![(1.0, 1)];
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(2));
        assert!(err.message.contains("common neighbor"), "{}", err.message);
    }

    #[test]
    fn rejects_round_gap() {
        let mut result = two_k4_v2_result();
        for step in &mut result.certificate.steps[2..] {
            step.position = SchedulePosition::Round(3);
        }
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(2));
        assert!(err.message.contains("skips round 2"), "{}", err.message);
    }

    #[test]
    fn rejects_interleaved_rounds() {
        // Round tags 1, 2, 1: the third step returns to a closed round.
        let mut result = two_k4_v2_result();
        result.certificate.steps.swap(1, 2);
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(2));
        assert!(err.message.contains("decreases"), "{}", err.message);
    }

    #[test]
    fn rejects_out_of_order_within_round() {
        // (4, 5) before (0, 1) replays cleanly but breaks the frozen
        // in-round order: equal values must go by ascending (v, u).
        let mut result = two_k4_v2_result();
        result.certificate.steps.swap(0, 1);
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("schedule order"), "{}", err.message);
    }

    #[test]
    fn rejects_first_round_not_one() {
        let mut result = two_k4_v2_result();
        result.certificate.steps[0].position = SchedulePosition::Round(2);
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("first step"), "{}", err.message);
    }

    #[test]
    fn rejects_round_zero() {
        let mut result = two_k4_v2_result();
        result.certificate.steps[0].position = SchedulePosition::Round(0);
        let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(0));
        assert!(err.message.contains("round number 0"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_algorithm_version() {
        for version in [0, 4] {
            let mut result = two_k4_v2_result();
            result.certificate.algorithm_version = version;
            let err = verify_dense(&two_k4_dense(), None, &result).unwrap_err();
            assert_eq!(err.step, None);
            assert!(
                err.message
                    .contains(&format!("unsupported algorithm version {version}")),
                "{}",
                err.message
            );
        }
    }

    #[test]
    fn accepts_budget_limited_adaptive_certificate_without_a_fixed_point() {
        let result = collapse_dense_adaptive(
            &triangle_dense(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H1).with_work_limit(0),
        )
        .unwrap();
        assert_eq!(
            result.certificate.completeness(),
            CollapseCompleteness::BudgetLimited
        );
        assert_eq!(verify_dense(&triangle_dense(), None, &result), Ok(()));
    }

    #[test]
    fn rejects_inconsistent_adaptive_metadata() {
        let complete = collapse_dense_adaptive(
            &triangle_dense(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H1),
        )
        .unwrap();

        let mut no_objective = complete.clone();
        no_objective.certificate.objective = None;
        let err = verify_dense(&triangle_dense(), None, &no_objective).unwrap_err();
        assert!(
            err.message.contains("no collapse objective"),
            "{}",
            err.message
        );

        let mut no_limit = complete.clone();
        no_limit.certificate.completeness = CollapseCompleteness::BudgetLimited;
        let err = verify_dense(&triangle_dense(), None, &no_limit).unwrap_err();
        assert!(err.message.contains("no work limit"), "{}", err.message);

        let mut under_limit = complete.clone();
        under_limit.certificate.completeness = CollapseCompleteness::BudgetLimited;
        under_limit.certificate.work_limit = Some(under_limit.certificate.work_used + 1);
        let err = verify_dense(&triangle_dense(), None, &under_limit).unwrap_err();
        assert!(
            err.message.contains("expected its limit"),
            "{}",
            err.message
        );

        let mut over_limit = complete;
        over_limit.certificate.work_limit = Some(0);
        let err = verify_dense(&triangle_dense(), None, &over_limit).unwrap_err();
        assert!(err.message.contains("exceeds its limit"), "{}", err.message);
    }

    #[test]
    fn rejects_adaptive_sequence_gap() {
        let mut result = collapse_dense_adaptive(
            &k4_dense(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H2),
        )
        .unwrap();
        result.certificate.steps[1].position = SchedulePosition::Sequence(3);
        let err = verify_dense(&k4_dense(), None, &result).unwrap_err();
        assert_eq!(err.step, Some(1));
        assert!(err.message.contains("sequence position"), "{}", err.message);
    }

    #[test]
    fn rejects_position_kind_for_each_algorithm_version() {
        let mut v1 = triangle_result();
        v1.certificate.steps[0].position = SchedulePosition::Round(1);
        let err = verify_dense(&triangle_dense(), None, &v1).unwrap_err();
        assert!(
            err.message.contains("not positioned in a pass"),
            "{}",
            err.message
        );

        let mut v2 = two_k4_v2_result();
        v2.certificate.steps[0].position = SchedulePosition::Pass(1);
        let err = verify_dense(&two_k4_dense(), None, &v2).unwrap_err();
        assert!(
            err.message.contains("not positioned in a round"),
            "{}",
            err.message
        );

        let mut v3 = collapse_dense_adaptive(
            &triangle_dense(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H1),
        )
        .unwrap();
        v3.certificate.steps[0].position = SchedulePosition::Pass(1);
        let err = verify_dense(&triangle_dense(), None, &v3).unwrap_err();
        assert!(
            err.message
                .contains("not positioned in an adaptive sequence"),
            "{}",
            err.message
        );
    }

    #[test]
    fn wide_independent_round_verifies_in_linear_time() {
        // Many disjoint unit K4s: round 1 removes one edge per block, so
        // the round is as wide as the block count. The independence check
        // must not compare every pair of steps.
        let blocks = 3000;
        let n = 4 * blocks;
        let mut triplets = Vec::new();
        for b in 0..blocks {
            let base = 4 * b;
            for i in 0..4 {
                for j in (i + 1)..4 {
                    triplets.push((base + i, base + j, 1.0));
                }
            }
        }
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        let result = collapse_sparse_rounds_parallel(&dist, None, 1).unwrap();
        assert!(result.certificate.steps().len() >= blocks);
        let start = std::time::Instant::now();
        verify_sparse(&dist, None, &result).unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "verifier took {:?} on a round of width {blocks}",
            start.elapsed()
        );
    }
}
