use std::collections::BTreeMap;

use super::super::{CollapseCertificate, CollapseCompleteness, CollapseObjective};
use super::model::{VerifyError, fail};
use super::replay::replay_version;
use super::witness::edge_removable;
use crate::SparseDistanceMatrix;

pub(super) fn verify_common(
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
    if let Some(limit) = cert.work_limit() {
        if cert.work_used() > limit {
            return Err(fail(
                None,
                format!(
                    "adaptive work used {} exceeds its limit {limit}",
                    cert.work_used()
                ),
            ));
        }
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
