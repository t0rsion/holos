//! Checked circular-coordinate families over class-extension atlases.

use crate::circular::{CircularCoordinate, CircularCoordinateParams, circular_coordinate};
use crate::{Error, Result};

use super::{BipersistenceModule, BipersistenceTerm, ClassExtensionKind, CohomologyClassAtlas};

/// One entry in a checked circular-coordinate family.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCoordinateFamilyEntry {
    /// Grid node.
    pub grade: super::Bigrade,
    /// Exact extension classification.
    pub extension: ClassExtensionKind,
    /// Coordinate for a unique extension.
    pub coordinate: Option<CircularCoordinate>,
}

/// Checked circle-valued coordinates for unique extensions of one class.
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
    /// Ambiguous and absent extensions have no coordinate. The call fails if
    /// automatic integral lifting or the harmonic solve fails at any unique
    /// node.
    pub fn circular_coordinate_family(
        &self,
        atlas: &CohomologyClassAtlas,
        params: CircularCoordinateParams,
    ) -> Result<CircularCoordinateFamily> {
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
        if self.modulus() == 2 {
            return Err(Error::InvalidInput(
                "automatic circular families require an odd prime modulus".into(),
            ));
        }
        let mut entries = Vec::with_capacity(atlas.extensions.len());
        for extension in &atlas.extensions {
            let coordinate = if extension.kind == ClassExtensionKind::Unique {
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
                let cocycle = crate::cocycle_from_ripser_terms(
                    &self.graphs[position],
                    self.modulus(),
                    0.0,
                    &rows,
                )?;
                Some(circular_coordinate(
                    &self.graphs[position],
                    &cocycle,
                    params,
                )?)
            } else {
                None
            };
            entries.push(CircularCoordinateFamilyEntry {
                grade: extension.grade,
                extension: extension.kind,
                coordinate,
            });
        }
        Ok(CircularCoordinateFamily {
            base_grade: atlas.base_grade,
            base_class: atlas.base_class.clone(),
            entries,
        })
    }
}
