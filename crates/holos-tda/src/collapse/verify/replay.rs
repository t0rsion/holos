use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::super::{CollapseCertificate, RemovalStep, SchedulePosition};
use super::model::{VerifyError, fail};
use super::witness::{check_edge_pair, check_live_value, check_witnesses};

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
            if let Some(&other) = in_round.get(&(x, y)) {
                if other != edge_index && conflict.is_none_or(|current| other < current) {
                    conflict = Some(other);
                }
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

pub(super) fn replay_version(
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
