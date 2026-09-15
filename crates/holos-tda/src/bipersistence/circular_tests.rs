use super::{BipersistenceModule, CircularCoordinateFamilyStatus, ClassExtensionKind};
use crate::bifiltration::{DegreeRipsBifiltration, DegreeRipsParams};
use crate::circular::CircularCoordinateParams;
use crate::{Bigrade, BipersistenceLimits, SparseDistanceMatrix};

fn cycle_graph(vertices: usize) -> SparseDistanceMatrix {
    let mut edges = (0..vertices - 1)
        .map(|u| (u, u + 1, 1.0))
        .collect::<Vec<_>>();
    edges.push((0, vertices - 1, 1.0));
    SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap()
}

fn module(graph: &SparseDistanceMatrix) -> BipersistenceModule {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(graph, DegreeRipsParams::default()).unwrap();
    BipersistenceModule::from_degree_rips(&degree_rips, 47, BipersistenceLimits::default()).unwrap()
}

fn atlas(module: &BipersistenceModule) -> crate::CohomologyClassAtlas {
    module
        .class_atlas(
            Bigrade::new(1, 5),
            &[crate::BipersistenceTerm {
                basis_index: 0,
                coefficient: 1,
            }],
        )
        .unwrap()
}

#[test]
fn family_validates_global_solver_limits_before_node_iteration() {
    let module = module(&cycle_graph(8));
    let error = module
        .circular_coordinate_family(
            &atlas(&module),
            CircularCoordinateParams::default().with_max_iterations(0),
        )
        .unwrap_err();
    assert!(error.to_string().contains("maximum iteration count"));
}

#[test]
fn family_rejects_modulus_two_before_node_iteration() {
    let graph = cycle_graph(8);
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let module =
        BipersistenceModule::from_degree_rips(&degree_rips, 2, BipersistenceLimits::default())
            .unwrap();
    let error = module
        .circular_coordinate_family(&atlas(&module), CircularCoordinateParams::default())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("automatic circular families require an odd prime modulus")
    );
}

#[test]
fn family_keeps_entries_when_a_harmonic_solve_fails() {
    let module = module(&cycle_graph(8));
    let atlas = atlas(&module);
    let family = module
        .circular_coordinate_family(
            &atlas,
            CircularCoordinateParams::default().with_max_iterations(1),
        )
        .unwrap();

    assert_eq!(family.entries.len(), atlas.extensions.len());
    assert!(family.entries.iter().any(|entry| {
        entry.extension == ClassExtensionKind::Unique
            && matches!(&entry.status, CircularCoordinateFamilyStatus::SolveFailed)
    }));
    assert!(family.entries.iter().all(|entry| match entry.extension {
        ClassExtensionKind::Unique => matches!(
            &entry.status,
            CircularCoordinateFamilyStatus::LiftFailed
                | CircularCoordinateFamilyStatus::SolveFailed
                | CircularCoordinateFamilyStatus::Success(_)
        ),
        ClassExtensionKind::Ambiguous | ClassExtensionKind::NoExtension => {
            matches!(&entry.status, CircularCoordinateFamilyStatus::NotAttempted)
        }
    }));
}
