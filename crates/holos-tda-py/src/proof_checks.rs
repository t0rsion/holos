//! Artifact verification bindings.

use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
pub(crate) fn verify_program_trace(
    py: Python<'_>,
    artifact: Vec<u8>,
) -> PyResult<(usize, usize, usize, usize)> {
    py.detach(|| {
        let artifact = ProgramTraceArtifact::decode(
            &artifact,
            program_trace_decode_limits(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let verified = artifact
            .verify(CertificateLimits::default())
            .map_err(display_err)?;
        let reused = verified
            .steps
            .iter()
            .filter(|step| step.mode == ProgramUpdateMode::Reused)
            .count();
        let repaired = verified
            .steps
            .iter()
            .filter(|step| step.mode == ProgramUpdateMode::Repaired)
            .count();
        let recompiled = verified.steps.len() - reused - repaired;
        Ok((verified.steps.len(), reused, repaired, recompiled))
    })
}

#[pyfunction]
pub(crate) fn verify_intervention(
    py: Python<'_>,
    artifact: Vec<u8>,
) -> PyResult<InterventionRecord> {
    py.detach(|| {
        let artifact = InterventionArtifact::decode(
            &artifact,
            InterventionDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let verified = artifact
            .verify(CertificateLimits::default())
            .map_err(display_err)?;
        let status = match verified.status {
            holos_tda::InterventionStatus::Optimal => "optimal",
            holos_tda::InterventionStatus::BoundedGap => "bounded_gap",
            holos_tda::InterventionStatus::BudgetLimited => "budget_limited",
            _ => "unknown",
        };
        Ok((
            status.into(),
            verified.target.to_string(),
            verified.lower_bound,
            Some(verified.upper_bound),
            verified
                .edits
                .into_iter()
                .map(|edit| ((edit.edge.u, edit.edge.v), edit.before, edit.after))
                .collect(),
            Some(to_program_result(verified.result)),
            None,
        ))
    })
}
