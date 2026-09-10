//! Shared persistence parameter parsing.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::{PyErr, PyResult};

use super::core::*;

// Keyword arguments match the Python signature.
#[allow(clippy::too_many_arguments)]
pub(crate) fn params(
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<RipsParams> {
    let mut p = RipsParams::new(max_dim).with_modulus(modulus);
    p.threshold = threshold;
    p.threads = threads.max(1);
    p.factorization = parse_factorization(factorization)?;
    p.collapse_edges = collapse_edges;
    p.collapse_schedule = parse_collapse_schedule(collapse_schedule)?;
    validate_collapse_settings(
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
        p.collapse_schedule,
    )?;
    let objective = parse_collapse_objective(collapse_objective)?;
    let mut adaptive = AdaptiveCollapseParams::new(objective);
    if let Some(limit) = collapse_work_limit {
        adaptive = adaptive.with_work_limit(limit);
    }
    p.adaptive_collapse = adaptive;
    Ok(p)
}

pub(crate) fn parse_factorization(value: &str) -> PyResult<GraphFactorization> {
    match value {
        "auto" => Ok(GraphFactorization::Auto),
        "off" => Ok(GraphFactorization::Off),
        "force" => Ok(GraphFactorization::Force),
        value => Err(PyValueError::new_err(format!(
            "factorization must be auto, off, or force, not {value}"
        ))),
    }
}

pub(crate) fn parse_collapse_schedule(value: &str) -> PyResult<CollapseSchedule> {
    match value {
        "serial" => Ok(CollapseSchedule::Serial),
        "ordered" => Ok(CollapseSchedule::Ordered),
        "rounds" => Ok(CollapseSchedule::Rounds),
        "adaptive" => Ok(CollapseSchedule::Adaptive),
        value => Err(PyValueError::new_err(format!(
            "collapse_schedule must be serial, ordered, rounds, or adaptive, not {value}"
        ))),
    }
}

pub(crate) fn validate_collapse_settings(
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
    schedule: CollapseSchedule,
) -> PyResult<()> {
    if !collapse_edges
        && (collapse_schedule != "serial"
            || collapse_objective != "h2"
            || collapse_work_limit.is_some())
    {
        return Err(PyValueError::new_err(
            "collapse settings require collapse_edges=True",
        ));
    }
    if (collapse_objective != "h2" || collapse_work_limit.is_some())
        && schedule != CollapseSchedule::Adaptive
    {
        return Err(PyValueError::new_err(
            "collapse_objective and collapse_work_limit require collapse_schedule='adaptive'",
        ));
    }
    Ok(())
}

pub(crate) fn parse_collapse_objective(value: &str) -> PyResult<CollapseObjective> {
    match value {
        "h1" => Ok(CollapseObjective::H1),
        "h2" => Ok(CollapseObjective::H2),
        value => Err(PyValueError::new_err(format!(
            "collapse_objective must be h1 or h2, not {value}"
        ))),
    }
}

pub(crate) fn to_err(e: holos_tda::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

pub(crate) fn display_err(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}
