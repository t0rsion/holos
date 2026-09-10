use super::Result;
use super::model::{
    EdgeWeightEdit, InterventionArtifact, InterventionError, InterventionStatus,
    VerifiedIntervention,
};
use super::primitives::{
    check_edit_shape, check_scalar_shape, edits_bits_equal, graph_bits_equal, maximum_edit,
};
use super::search::{apply_edits, continued_space_dies_by, destroyer_edits};
use crate::{
    CertificateLimits, IntervalGroupId, ProgramTraceArtifact, ProgramUpdateMode,
    SparseDistanceMatrix, VerifiedProgramTrace,
};

impl InterventionArtifact {
    /// Check the edit, bounds, target continuation, and nested trace.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedIntervention, InterventionError> {
        self.check_shape()?;
        let verified = verify_trace(&self.trace, certificate_limits)?;
        check_trace_cardinality(&self.trace, &verified)?;
        let initial = self.trace.initial_graph();
        let updated = self.trace.steps()[0].graph();
        check_applied_edits(initial, updated, &self.edits)?;
        let space = find_target_space(&verified, self.target)?;
        check_target_interval(space, self.target_scale)?;
        check_target_death(&verified, self.target, self.target_scale)?;
        check_upper_bound(&self.edits, self.upper_bound)?;
        check_intervention_status(self, &verified, initial, space)?;
        Ok(VerifiedIntervention {
            status: self.status,
            target: self.target,
            target_scale: self.target_scale,
            lower_bound: self.lower_bound,
            upper_bound: self.upper_bound,
            edits: self.edits.clone(),
            result: verified.final_program.result().clone(),
        })
    }

    pub(super) fn check_shape(&self) -> std::result::Result<(), InterventionError> {
        check_scalar_shape(self)?;
        check_edit_shape(&self.edits)
    }
}
fn verify_trace(
    trace: &ProgramTraceArtifact,
    certificate_limits: CertificateLimits,
) -> Result<VerifiedProgramTrace, InterventionError> {
    trace
        .verify(certificate_limits)
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn check_trace_cardinality(
    trace: &ProgramTraceArtifact,
    verified: &VerifiedProgramTrace,
) -> Result<(), InterventionError> {
    if trace.steps().len() != 1 || verified.steps.len() != 1 {
        return Err(InterventionError::new(
            "intervention trace must contain exactly one update",
        ));
    }
    Ok(())
}

fn check_applied_edits(
    initial: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    edits: &[EdgeWeightEdit],
) -> Result<(), InterventionError> {
    let applied =
        apply_edits(initial, edits).map_err(|error| InterventionError::new(error.to_string()))?;
    if !graph_bits_equal(&applied, updated) {
        return Err(InterventionError::new(
            "edge edits do not reproduce the traced graph",
        ));
    }
    Ok(())
}

fn find_target_space(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
) -> Result<&crate::PersistentClassSpace, InterventionError> {
    verified
        .initial_result
        .spaces
        .iter()
        .find(|space| space.id == target)
        .ok_or_else(|| InterventionError::new("target space is absent initially"))
}

fn check_target_interval(
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<(), InterventionError> {
    let outside = space.interval.is_essential()
        || target_scale <= space.interval.birth
        || target_scale >= space.interval.death;
    if outside {
        return Err(InterventionError::new(
            "target scale is outside the finite target interval",
        ));
    }
    Ok(())
}

fn check_target_death(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<(), InterventionError> {
    if !continued_space_dies_by(verified, target, target_scale) {
        return Err(InterventionError::new(
            "the continued target space does not die by the requested scale",
        ));
    }
    Ok(())
}

fn check_upper_bound(edits: &[EdgeWeightEdit], upper_bound: f64) -> Result<(), InterventionError> {
    if maximum_edit(edits).to_bits() != upper_bound.to_bits() {
        return Err(InterventionError::new(
            "upper bound differs from the applied edit",
        ));
    }
    Ok(())
}

fn check_intervention_status(
    artifact: &InterventionArtifact,
    verified: &VerifiedProgramTrace,
    initial: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
) -> Result<(), InterventionError> {
    match artifact.status {
        InterventionStatus::Optimal => check_optimal_claim(artifact, verified, initial, space),
        InterventionStatus::BoundedGap => check_bounded_gap_claim(artifact.lower_bound),
        InterventionStatus::BudgetLimited => Err(InterventionError::new(
            "a budget-limited search has no feasible artifact",
        )),
    }
}

fn check_optimal_claim(
    artifact: &InterventionArtifact,
    verified: &VerifiedProgramTrace,
    initial: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
) -> Result<(), InterventionError> {
    if verified.steps[0].mode != ProgramUpdateMode::Reused {
        return Err(InterventionError::new(
            "an optimal claim left the checked reduction region",
        ));
    }
    let expected = destroyer_edits(initial, space, artifact.target_scale)
        .map_err(|error| InterventionError::new(error.to_string()))?;
    let lower = space.interval.death - artifact.target_scale;
    check_optimal_edit_and_bounds(artifact, &expected, lower)
}

fn check_optimal_edit_and_bounds(
    artifact: &InterventionArtifact,
    expected: &[EdgeWeightEdit],
    lower: f64,
) -> Result<(), InterventionError> {
    let differs = !edits_bits_equal(expected, &artifact.edits)
        || lower.to_bits() != artifact.lower_bound.to_bits()
        || lower.to_bits() != artifact.upper_bound.to_bits();
    if differs {
        return Err(InterventionError::new(
            "optimal edit or matching bound is not canonical",
        ));
    }
    Ok(())
}

fn check_bounded_gap_claim(lower_bound: f64) -> Result<(), InterventionError> {
    if lower_bound.to_bits() != 0 {
        return Err(InterventionError::new(
            "bounded-gap lower bound must be zero",
        ));
    }
    Ok(())
}
