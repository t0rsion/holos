//! Exact finite H1 modules over a degree-Rips parameter grid.
//!
//! The module stores every canonical cohomology space and every cover
//! restriction. This is a complete representation on the declared finite
//! grid. It is not a minimal bigraded presentation or a free resolution.

mod atlas;
mod circular;
mod linear;
mod module;
mod queries;

pub use atlas::{ClassExtension, ClassExtensionKind, ClassExtensionRegion, CohomologyClassAtlas};
pub use circular::{CircularCoordinateFamily, CircularCoordinateFamilyEntry};
pub use module::BipersistenceModule;

use crate::bifiltration::Bigrade;
use crate::cohomology::{CohomologyLimits, CohomologySpaceId};
use crate::{Error, Result};

/// Resource limits for one finite bipersistence module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BipersistenceLimits {
    /// Largest accepted grid-node count.
    pub max_nodes: usize,
    /// Largest accepted cover-map count.
    pub max_cover_maps: usize,
    /// Largest sum of cohomology ranks across all nodes.
    pub max_total_rank: usize,
    /// Largest total nonzero coefficient count across cover maps.
    pub max_map_terms: usize,
    /// Largest direct-sum dimension used by one exact query.
    pub max_linear_variables: usize,
    /// Largest sparse coefficient count admitted by one exact query.
    pub max_linear_terms: usize,
    /// Limits for each canonical cohomology space.
    pub cohomology: CohomologyLimits,
}

impl Default for BipersistenceLimits {
    fn default() -> Self {
        Self {
            max_nodes: 100_000,
            max_cover_maps: 200_000,
            max_total_rank: 10_000_000,
            max_map_terms: 100_000_000,
            max_linear_variables: 100_000,
            max_linear_terms: 100_000_000,
            cohomology: CohomologyLimits::default(),
        }
    }
}

/// One canonical vector-space node of a finite bipersistence module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BipersistenceNode {
    /// Position in the parameter grid.
    pub grade: Bigrade,
    /// Content identifier of the canonical cohomology space.
    pub space: CohomologySpaceId,
    /// Dimension of `H¹` at this grade.
    pub rank: usize,
}

/// One nonzero coefficient in canonical basis coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BipersistenceTerm {
    /// Zero-based canonical basis position.
    pub basis_index: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Image of one source basis vector under a module map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BipersistenceMapColumn {
    /// Zero-based source basis position.
    pub source_basis_index: usize,
    /// Nonzero target coordinates in ascending basis order.
    pub image: Vec<BipersistenceTerm>,
}

/// Exact contravariant `H¹` map between comparable grid nodes.
///
/// `lower_grade` precedes `upper_grade`. The map runs from the cohomology at
/// `upper_grade` to the cohomology at `lower_grade`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BipersistenceMap {
    /// Smaller filtration grade and map target.
    pub lower_grade: Bigrade,
    /// Larger filtration grade and map source.
    pub upper_grade: Bigrade,
    /// Canonical source space at `upper_grade`.
    pub source_space: CohomologySpaceId,
    /// Canonical target space at `lower_grade`.
    pub target_space: CohomologySpaceId,
    /// Rank of the linear map.
    pub rank: usize,
    /// Columns in source basis order.
    pub columns: Vec<BipersistenceMapColumn>,
}

/// One closed rectangle in the finite parameter grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BipersistenceRectangle {
    /// Coordinatewise minimum corner.
    pub lower: Bigrade,
    /// Coordinatewise maximum corner.
    pub upper: Bigrade,
}

impl BipersistenceRectangle {
    /// Construct a rectangle with comparable corners.
    pub fn new(lower: Bigrade, upper: Bigrade) -> Result<Self> {
        if !lower.precedes(upper) {
            return Err(Error::InvalidInput(
                "a bipersistence rectangle needs comparable corners".into(),
            ));
        }
        Ok(Self { lower, upper })
    }
}

/// One finite connected region of the parameter grid.
///
/// Grades are canonicalized in lexicographic order. The module query checks
/// that every grade lies on its grid and that the comparability graph is
/// connected. The region need not have a minimum or a maximum.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BipersistenceRegion {
    grades: Vec<Bigrade>,
}

impl BipersistenceRegion {
    /// Construct a nonempty region from distinct grades.
    pub fn new(mut grades: Vec<Bigrade>) -> Result<Self> {
        grades.sort_unstable();
        if grades.is_empty() {
            return Err(Error::InvalidInput(
                "a bipersistence region needs at least one grade".into(),
            ));
        }
        if grades.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::InvalidInput(
                "bipersistence region grades must be distinct".into(),
            ));
        }
        Ok(Self { grades })
    }

    /// Canonical grades in lexicographic order.
    pub fn grades(&self) -> &[Bigrade] {
        &self.grades
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SparseDistanceMatrix;
    use crate::bifiltration::{DegreeRipsBifiltration, DegreeRipsParams};
    use crate::circular::CircularCoordinateParams;

    fn square_with_diagonals() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap()
    }

    fn module(graph: &SparseDistanceMatrix, modulus: u32) -> BipersistenceModule {
        let degree_rips =
            DegreeRipsBifiltration::from_graph(graph, DegreeRipsParams::default()).unwrap();
        BipersistenceModule::from_degree_rips(&degree_rips, modulus, BipersistenceLimits::default())
            .unwrap()
    }

    #[test]
    fn builds_commuting_degree_rips_module() {
        let module = module(&square_with_diagonals(), 47);
        assert_eq!(module.scales(), &[0.0, 1.0, 2.0]);
        assert_eq!(module.minimum_degrees(), &[3, 2, 1, 0]);
        assert_eq!(module.nodes().len(), 12);
        assert_eq!(module.cover_maps().len(), 17);
        assert_eq!(module.node(Bigrade::new(1, 1)).unwrap().rank, 1);
        assert_eq!(module.node(Bigrade::new(2, 0)).unwrap().rank, 0);
        assert_eq!(
            module
                .map_rank(Bigrade::new(1, 1), Bigrade::new(2, 1))
                .unwrap(),
            0
        );
    }

    #[test]
    fn rectangle_rank_agrees_with_constant_cycle() {
        let cycle = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let module = module(&cycle, 47);
        let rectangle =
            BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(1, 3)).unwrap();
        assert_eq!(module.rectangle_rank(rectangle).unwrap(), 1);
        assert_eq!(
            module.rectangle_rank(rectangle).unwrap(),
            module.map_rank(rectangle.lower, rectangle.upper).unwrap()
        );
    }

    #[test]
    fn connected_region_without_extrema_has_generalized_rank() {
        let cycle = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
            &cycle,
            vec![1.0, 2.0, 3.0],
            vec![2, 1, 0],
            DegreeRipsParams {
                threshold: Some(3.0),
                ..DegreeRipsParams::default()
            },
        )
        .unwrap();
        let module =
            BipersistenceModule::from_degree_rips(&degree_rips, 47, BipersistenceLimits::default())
                .unwrap();
        let region = BipersistenceRegion::new(vec![
            Bigrade::new(0, 1),
            Bigrade::new(1, 1),
            Bigrade::new(1, 0),
            Bigrade::new(2, 0),
        ])
        .unwrap();
        assert_eq!(module.region_rank(&region).unwrap(), 1);

        let disconnected =
            BipersistenceRegion::new(vec![Bigrade::new(0, 1), Bigrade::new(1, 0)]).unwrap();
        assert!(module.region_rank(&disconnected).is_err());
    }

    #[test]
    fn class_atlas_finds_unique_and_absent_extensions() {
        let module = module(&square_with_diagonals(), 47);
        let atlas = module
            .class_atlas(
                Bigrade::new(1, 1),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient: 1,
                }],
            )
            .unwrap();
        assert_eq!(atlas.extensions[0].kind, ClassExtensionKind::Unique);
        assert!(
            atlas
                .extensions
                .iter()
                .any(|extension| extension.kind == ClassExtensionKind::NoExtension)
        );
    }

    #[test]
    fn class_atlas_reports_ambiguous_extensions() {
        let graph = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 2.0),
                (4, 5, 2.0),
                (5, 6, 2.0),
                (0, 6, 2.0),
            ],
        )
        .unwrap();
        let module = module(&graph, 47);
        let atlas = module
            .class_atlas(
                Bigrade::new(1, 6),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient: 1,
                }],
            )
            .unwrap();
        let extension = atlas
            .extensions
            .iter()
            .find(|extension| extension.grade == Bigrade::new(2, 6))
            .unwrap();
        assert_eq!(extension.kind, ClassExtensionKind::Ambiguous);
        assert_eq!(extension.ambiguity.len(), 1);
    }

    #[test]
    fn circular_family_covers_unique_cycle_extensions() {
        let cycle = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let module = module(&cycle, 47);
        let atlas = module
            .class_atlas(
                Bigrade::new(1, 1),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient: 1,
                }],
            )
            .unwrap();
        let family = module
            .circular_coordinate_family(&atlas, CircularCoordinateParams::default())
            .unwrap();
        assert!(family.entries.iter().all(|entry| {
            (entry.extension == ClassExtensionKind::Unique) == entry.coordinate.is_some()
        }));

        let scaled = module
            .class_atlas(
                Bigrade::new(1, 1),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient: 2,
                }],
            )
            .unwrap();
        let scaled_family = module
            .circular_coordinate_family(&scaled, CircularCoordinateParams::default())
            .unwrap();
        assert!(
            scaled_family
                .entries
                .iter()
                .any(|entry| entry.coordinate.is_some())
        );
    }

    #[test]
    fn generalized_rank_matches_map_rank_on_two_node_rectangles() {
        let module = module(&square_with_diagonals(), 47);
        for map in module.cover_maps() {
            let rectangle = BipersistenceRectangle::new(map.lower_grade, map.upper_grade).unwrap();
            assert_eq!(module.rectangle_rank(rectangle).unwrap(), map.rank);
        }
    }
}
