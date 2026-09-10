use crate::program::class_continuation;
use crate::{
    CertificateLimits, ClassCorrespondence, PersistenceProgram, ProgramArtifact, ProgramUpdateMode,
    SparseDistanceMatrix,
};

use super::codec::{diagram_bits_equal, program_artifact_error};
use super::model::{ProgramTraceError, ProgramTraceStep};

pub(crate) fn verify_program_artifact(
    artifact: &ProgramArtifact,
    graph: &SparseDistanceMatrix,
    certificate_limits: CertificateLimits,
) -> Result<PersistenceProgram, ProgramTraceError> {
    artifact
        .verify(graph, certificate_limits)
        .map_err(program_artifact_error)
}

pub(crate) fn replay_step(
    program: &mut PersistenceProgram,
    step: &ProgramTraceStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<crate::ProgramUpdate, ProgramTraceError> {
    if step.mode == ProgramUpdateMode::Reused {
        return program
            .advance_reused(&step.graph)
            .map_err(|error| ProgramTraceError::new(error.to_string()));
    }
    replay_checkpoint_step(program, step, index, certificate_limits)
}

pub(crate) fn replay_checkpoint_step(
    program: &mut PersistenceProgram,
    step: &ProgramTraceStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<crate::ProgramUpdate, ProgramTraceError> {
    let checkpoint = step.checkpoint.as_ref().ok_or_else(|| {
        ProgramTraceError::new(format!("step {index} has no required checkpoint"))
    })?;
    let replacement = verify_program_artifact(checkpoint, &step.graph, certificate_limits)?;
    let (mode, events, work) = program
        .preview_update(&step.graph, replacement.states().len())
        .map_err(|error| ProgramTraceError::new(error.to_string()))?;
    let continuation = class_continuation(&program.result().spaces, &replacement.result().spaces);
    let correspondence = replay_correspondence(program, &step.graph, &replacement)?;
    let result = replacement.result().clone();
    *program = replacement;
    Ok(crate::ProgramUpdate {
        result,
        mode,
        events,
        continuation,
        correspondence,
        work,
    })
}

pub(crate) fn replay_correspondence(
    program: &PersistenceProgram,
    graph: &SparseDistanceMatrix,
    replacement: &PersistenceProgram,
) -> Result<Vec<ClassCorrespondence>, ProgramTraceError> {
    crate::class_correspondences(
        program.current_graph(),
        &program.result().spaces,
        graph,
        &replacement.result().spaces,
        replacement.params().modulus,
    )
    .map_err(|error| ProgramTraceError::new(error.to_string()))
}

pub(crate) fn check_replayed_step(
    checked: &crate::ProgramUpdate,
    declared: &ProgramTraceStep,
    index: usize,
) -> Result<(), ProgramTraceError> {
    let difference = if checked.mode != declared.mode {
        Some("mode".to_string())
    } else if checked.work != declared.work {
        Some(format!(
            "work counters differ: declared {:?}, replayed {:?}",
            declared.work, checked.work
        ))
    } else if checked.events != declared.events {
        Some("events".to_string())
    } else if checked.continuation != declared.continuation {
        Some("class continuation".to_string())
    } else if checked.correspondence != declared.correspondence {
        Some("class correspondence".to_string())
    } else if !diagram_bits_equal(&checked.result.diagram, &declared.diagram) {
        Some("diagram".to_string())
    } else {
        None
    };
    if let Some(difference) = difference {
        return Err(ProgramTraceError::new(format!(
            "step {index} {difference} in independent replay"
        )));
    }
    Ok(())
}

pub(crate) fn check_step_shape(
    step: &ProgramTraceStep,
) -> std::result::Result<(), ProgramTraceError> {
    match (step.mode, step.checkpoint.is_some()) {
        (ProgramUpdateMode::Reused, false)
        | (ProgramUpdateMode::Repaired, true)
        | (ProgramUpdateMode::Recompiled, true) => Ok(()),
        (ProgramUpdateMode::Reused, true) => Err(ProgramTraceError::new(
            "a reused step contains a redundant checkpoint",
        )),
        (_, false) => Err(ProgramTraceError::new(
            "a repaired or recompiled step has no checkpoint",
        )),
    }
}
