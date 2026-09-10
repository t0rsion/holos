use crate::{
    AtlasArtifact, AtlasEvaluation, CertificateLimits, SparseDistanceMatrix, TopologyEvent,
    UpdateMode,
};

use super::model::{TrajectoryError, TrajectoryStep, VerifiedTrajectoryStep};

pub(super) fn verify_atlas(
    artifact: &AtlasArtifact,
    input: &SparseDistanceMatrix,
    certificate_limits: CertificateLimits,
) -> Result<crate::PersistenceAtlas, TrajectoryError> {
    artifact
        .verify(input, certificate_limits)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

pub(super) fn verify_trajectory_step(
    atlas: &mut crate::PersistenceAtlas,
    step: &TrajectoryStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<VerifiedTrajectoryStep, TrajectoryError> {
    let events = atlas.events(&step.input);
    check_step_events(&events, &step.events, index)?;
    let mode = mode_for_events(&events);
    check_step_mode(step.mode, mode, index)?;
    let evaluation = evaluate_trajectory_step(atlas, step, mode, index, certificate_limits)?;
    Ok(VerifiedTrajectoryStep {
        mode,
        events,
        evaluation,
    })
}

fn check_step_events(
    actual: &[TopologyEvent],
    recorded: &[TopologyEvent],
    index: usize,
) -> Result<(), TrajectoryError> {
    if !events_bits_equal(actual, recorded) {
        return Err(TrajectoryError::new(format!(
            "step {index} events differ from the current atlas"
        )));
    }
    Ok(())
}

fn mode_for_events(events: &[TopologyEvent]) -> UpdateMode {
    if events.is_empty() {
        UpdateMode::Reused
    } else {
        UpdateMode::Recomputed
    }
}

fn check_step_mode(
    recorded: UpdateMode,
    actual: UpdateMode,
    index: usize,
) -> Result<(), TrajectoryError> {
    if recorded != actual {
        return Err(TrajectoryError::new(format!(
            "step {index} mode differs from its events"
        )));
    }
    Ok(())
}

fn evaluate_trajectory_step(
    atlas: &mut crate::PersistenceAtlas,
    step: &TrajectoryStep,
    mode: UpdateMode,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<AtlasEvaluation, TrajectoryError> {
    match (&step.checkpoint, mode) {
        (None, UpdateMode::Reused) => atlas
            .evaluate(&step.input)
            .map_err(|error| TrajectoryError::new(error.to_string())),
        (Some(checkpoint), UpdateMode::Recomputed) => {
            *atlas = verify_atlas(checkpoint, &step.input, certificate_limits)?;
            atlas
                .evaluate(&step.input)
                .map_err(|error| TrajectoryError::new(error.to_string()))
        }
        _ => Err(TrajectoryError::new(format!(
            "step {index} checkpoint does not match its mode"
        ))),
    }
}

fn events_bits_equal(a: &[TopologyEvent], b: &[TopologyEvent]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.kind == b.kind
                && a.first == b.first
                && a.second == b.second
                && optional_f64_bits_equal(a.old_first, b.old_first)
                && optional_f64_bits_equal(a.new_first, b.new_first)
                && optional_f64_bits_equal(a.old_second, b.old_second)
                && optional_f64_bits_equal(a.new_second, b.new_second)
        })
}

fn optional_f64_bits_equal(a: Option<f64>, b: Option<f64>) -> bool {
    a.map(f64::to_bits) == b.map(f64::to_bits)
}
