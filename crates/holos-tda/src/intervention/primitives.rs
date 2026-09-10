use super::model::{EdgeWeightEdit, InterventionArtifact, InterventionError};
use crate::SparseDistanceMatrix;

pub(super) fn check_scalar_shape(artifact: &InterventionArtifact) -> Result<(), InterventionError> {
    let invalid = !artifact.target_scale.is_finite()
        || artifact.target_scale < 0.0
        || !artifact.lower_bound.is_finite()
        || artifact.lower_bound < 0.0
        || !artifact.upper_bound.is_finite()
        || artifact.upper_bound < artifact.lower_bound;
    if invalid {
        return Err(InterventionError::new(
            "target scale or bounds are not canonical",
        ));
    }
    Ok(())
}

pub(super) fn check_edit_shape(edits: &[EdgeWeightEdit]) -> Result<(), InterventionError> {
    let invalid = edits.is_empty()
        || edits.windows(2).any(|pair| pair[0].edge >= pair[1].edge)
        || edits.iter().any(|edit| !edit_is_canonical(edit));
    if invalid {
        return Err(InterventionError::new(
            "edge edits are not canonical strict decreases",
        ));
    }
    Ok(())
}

pub(super) fn edit_is_canonical(edit: &EdgeWeightEdit) -> bool {
    edit.edge.u < edit.edge.v
        && edit.before.is_finite()
        && edit.after.is_finite()
        && edit.after >= 0.0
        && edit.after < edit.before
}
pub(super) fn maximum_edit(edits: &[EdgeWeightEdit]) -> f64 {
    edits
        .iter()
        .map(|edit| edit.before - edit.after)
        .fold(0.0f64, f64::max)
}

pub(super) fn graph_bits_equal(a: &SparseDistanceMatrix, b: &SparseDistanceMatrix) -> bool {
    a.len() == b.len()
        && a.num_edges() == b.num_edges()
        && a.edges()
            .zip(b.edges())
            .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2.to_bits() == b.2.to_bits())
}

pub(super) fn edits_bits_equal(a: &[EdgeWeightEdit], b: &[EdgeWeightEdit]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.edge == b.edge
                && a.before.to_bits() == b.before.to_bits()
                && a.after.to_bits() == b.after.to_bits()
        })
}
