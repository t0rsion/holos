use std::collections::BTreeMap;

use super::super::RemovalStep;
use super::model::{VerifyError, fail};

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
pub(super) fn edge_removable(
    adj: &[BTreeMap<usize, f64>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
) -> bool {
    let cands = candidates(adj, u, v, a, terminal);
    critical_values(a, &cands).iter().all(|&t| {
        cands
            .iter()
            .any(|&(w, b)| b <= t && vertex_dominates(adj, u, v, a, &cands, w, t))
    })
}

/// Check that the step names a valid vertex pair; return the endpoints and
/// the recorded value.
pub(super) fn check_edge_pair(
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
pub(super) fn check_live_value(
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

/// Validate the step's witness segments against `adj` with the full frozen
/// selection rule.
pub(super) fn check_witnesses(
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
