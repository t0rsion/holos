use crate::circular::{
    CircularCoordinateFailure, CircularCoordinateParams, IntegralCocycleTerm, selected_coordinate,
};
use crate::{PersistentClassArtifact, Result};

use super::model::PersistentCoordinateArtifact;

impl PersistentCoordinateArtifact {
    /// Build a selected coordinate from an immutable persistent-class artifact.
    ///
    /// With `integral_lift` set to `None`, centered lifting searches the odd
    /// prime field. A supplied lift also supports modulus two.
    pub fn build(
        class_artifact: &PersistentClassArtifact,
        params: CircularCoordinateParams,
        integral_lift: Option<&[IntegralCocycleTerm]>,
    ) -> Result<Self> {
        let coordinate = selected_coordinate(
            class_artifact.source(),
            &class_artifact.class().cocycle,
            integral_lift,
            params,
        )
        .map_err(CircularCoordinateFailure::into_error)?;
        Ok(Self {
            class_artifact: class_artifact.clone(),
            coordinate,
        })
    }
}
