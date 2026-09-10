use super::super::CollapsedRips;
use super::super::wire::{CollapseArtifact, graph_digest};
use super::model::{VerifyError, fail};
use super::validate::verify_common;
use crate::{DistanceMatrix, SparseDistanceMatrix};

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
