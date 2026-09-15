//! Checked circular-coordinate families over class-extension atlases.

use crate::circular::{
    CircularCoordinate, CircularCoordinateFailure, CircularCoordinateParams,
    circular_coordinate_with_failure_stage, validate_params,
};
use crate::{Error, Result};

use super::{BipersistenceModule, BipersistenceTerm, ClassExtensionKind, CohomologyClassAtlas};

/// Computational outcome for one class extension.
#[derive(Debug, Clone, PartialEq)]
pub enum CircularCoordinateFamilyStatus {
    /// No circular-coordinate computation was attempted.
    NotAttempted,
    /// Automatic integral lifting did not produce a checked lift.
    ///
    /// This status reports a bounded computation result. It does not prove
    /// that the class has no integral lift.
    LiftFailed,
    /// A checked lift was available, but harmonic coordinate construction failed.
    SolveFailed,
    /// The checked circular coordinate was computed.
    Success(Box<CircularCoordinate>),
}

impl CircularCoordinateFamilyStatus {
    /// Return the coordinate when computation succeeded.
    pub fn coordinate(&self) -> Option<&CircularCoordinate> {
        match self {
            Self::Success(coordinate) => Some(coordinate),
            Self::NotAttempted | Self::LiftFailed | Self::SolveFailed => None,
        }
    }
}

/// One entry in a checked circular-coordinate family.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCoordinateFamilyEntry {
    /// Grid node.
    pub grade: super::Bigrade,
    /// Exact extension classification.
    pub extension: ClassExtensionKind,
    /// Computational outcome for this extension.
    pub status: CircularCoordinateFamilyStatus,
}

/// Checked circle-valued coordinates and outcomes for one class extension family.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCoordinateFamily {
    /// Grade of the selected class.
    pub base_grade: super::Bigrade,
    /// Selected class in the base node's canonical basis.
    pub base_class: Vec<BipersistenceTerm>,
    /// One entry at every grade in the upper parameter cone.
    pub entries: Vec<CircularCoordinateFamilyEntry>,
}

impl BipersistenceModule {
    /// Compute checked circular coordinates for every unique atlas extension.
    ///
    /// Ambiguous and absent extensions have status [`CircularCoordinateFamilyStatus::NotAttempted`].
    /// Every unique extension has a lift failure, solve failure, or success
    /// status. A failed computation is reported at its node, and does not
    /// discard coordinates computed at other nodes.
    /// Automatic family construction requires an odd prime coefficient modulus.
    /// Modulus two requires a caller-supplied integral lift and is rejected
    /// before any node computation.
    pub fn circular_coordinate_family(
        &self,
        atlas: &CohomologyClassAtlas,
        params: CircularCoordinateParams,
    ) -> Result<CircularCoordinateFamily> {
        validate_params(params)?;
        if self.modulus() == 2 {
            return Err(Error::InvalidInput(
                "automatic circular families require an odd prime modulus".into(),
            ));
        }
        if atlas.base_grade
            != atlas
                .extensions
                .first()
                .map(|extension| extension.grade)
                .unwrap_or(atlas.base_grade)
        {
            return Err(Error::InvalidInput(
                "the class atlas is not in canonical grid order".into(),
            ));
        }
        let rebuilt = self.class_atlas(atlas.base_grade, &atlas.base_class)?;
        if rebuilt != *atlas {
            return Err(Error::InvalidInput(
                "the class atlas belongs to a different bipersistence module".into(),
            ));
        }
        let mut entries = Vec::with_capacity(atlas.extensions.len());
        for extension in &atlas.extensions {
            let status = if extension.kind == ClassExtensionKind::Unique {
                self.coordinate_status(extension, params)?
            } else {
                CircularCoordinateFamilyStatus::NotAttempted
            };
            entries.push(CircularCoordinateFamilyEntry {
                grade: extension.grade,
                extension: extension.kind,
                status,
            });
        }
        Ok(CircularCoordinateFamily {
            base_grade: atlas.base_grade,
            base_class: atlas.base_class.clone(),
            entries,
        })
    }

    fn coordinate_status(
        &self,
        extension: &super::ClassExtension,
        params: CircularCoordinateParams,
    ) -> Result<CircularCoordinateFamilyStatus> {
        let position = self.node_index(extension.grade);
        let coordinates = extension
            .class
            .iter()
            .map(|term| (term.basis_index, term.coefficient))
            .collect::<Vec<_>>();
        let rows = self.spaces[position]
            .cocycle_from_coordinates(&coordinates)?
            .into_iter()
            .map(|term| (term.simplex[0], term.simplex[1], term.coefficient))
            .collect::<Vec<_>>();
        let cocycle =
            crate::cocycle_from_ripser_terms(&self.graphs[position], self.modulus(), 0.0, &rows)?;
        let status = match circular_coordinate_with_failure_stage(
            &self.graphs[position],
            &cocycle,
            params,
        ) {
            Ok(coordinate) => CircularCoordinateFamilyStatus::Success(Box::new(coordinate)),
            Err(CircularCoordinateFailure::Lift(_)) => CircularCoordinateFamilyStatus::LiftFailed,
            Err(CircularCoordinateFailure::Solve(_)) => CircularCoordinateFamilyStatus::SolveFailed,
            Err(CircularCoordinateFailure::Other(error)) => return Err(error),
        };
        Ok(status)
    }
}

#[cfg(test)]
#[path = "circular_tests.rs"]
mod tests;
