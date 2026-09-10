use super::*;
use crate::{
    Bigrade, BipersistenceRegion, BipersistenceTerm, CircularCoordinateParams,
    DegreeRipsBifiltration, DegreeRipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{BipersistenceProofLimits, verify_bipersistence};

fn graph() -> SparseDistanceMatrix {
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

#[test]
fn artifact_round_trips_and_independent_checker_accepts() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    artifact
        .record_rectangle(
            &module,
            crate::BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(2, 3)).unwrap(),
            limits,
        )
        .unwrap();
    artifact
        .record_region(
            &module,
            BipersistenceRegion::new(vec![
                Bigrade::new(0, 2),
                Bigrade::new(1, 2),
                Bigrade::new(1, 1),
                Bigrade::new(2, 1),
            ])
            .unwrap(),
            limits,
        )
        .unwrap();
    let atlas = module
        .class_atlas(
            Bigrade::new(1, 1),
            &[BipersistenceTerm {
                basis_index: 0,
                coefficient: 2,
            }],
        )
        .unwrap();
    artifact
        .record_class_atlas(&module, &atlas, limits)
        .unwrap();
    artifact
        .record_circular_family(&module, &atlas, CircularCoordinateParams::default(), limits)
        .unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let decoded = BipersistenceArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    assert_eq!(decoded.summary().rectangles, 1);
    assert_eq!(decoded.summary().regions, 1);
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.nodes, module.nodes().len());
    assert_eq!(checked.rectangles, 1);
    assert_eq!(checked.regions, 1);
    assert_eq!(checked.class_atlases, 1);
    assert_eq!(checked.circular_families, 1);
}

#[test]
fn declared_grid_round_trips_and_independent_checker_accepts() {
    let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
        &graph(),
        vec![1.0, 2.0],
        vec![2, 0],
        DegreeRipsParams {
            threshold: Some(2.0),
            ..DegreeRipsParams::default()
        },
    )
    .unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    artifact
        .record_rectangle(
            &module,
            crate::BipersistenceRectangle::new(Bigrade::new(0, 0), Bigrade::new(1, 1)).unwrap(),
            limits,
        )
        .unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let decoded = BipersistenceArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.scales, 2);
    assert_eq!(checked.density_levels, 2);
    assert_eq!(checked.nodes, 4);
}

#[test]
fn artifact_rejects_changed_claim() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (artifact, _) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    let mut bytes = artifact.encode(limits).unwrap();
    let position = bytes.len() - 33;
    bytes[position] ^= 1;
    assert!(BipersistenceArtifact::decode(&bytes, limits).is_err());
    assert!(verify_bipersistence(&bytes, BipersistenceProofLimits::default()).is_err());
}

fn artifact_and_module() -> (BipersistenceArtifact, crate::BipersistenceModule) {
    let filtration =
        DegreeRipsBifiltration::from_graph(&graph(), DegreeRipsParams::default()).unwrap();
    BipersistenceArtifact::build(&filtration, 47, BipersistenceArtifactLimits::default()).unwrap()
}

fn assert_collection_limits(
    mut artifact: BipersistenceArtifact,
    limits_for: impl Fn(usize) -> BipersistenceArtifactLimits,
    record: impl Fn(&mut BipersistenceArtifact, bool, BipersistenceArtifactLimits) -> crate::Result<()>,
) {
    for capacity in [0, 1] {
        let limits = limits_for(capacity);
        if capacity == 1 {
            record(&mut artifact, false, limits).unwrap();
        }
        let bytes = artifact.encode(limits).unwrap();
        let digest = artifact.digest;
        let error = record(&mut artifact, capacity == 1, limits).unwrap_err();
        assert!(
            error.to_string().contains("count exceeds the limit"),
            "{error}"
        );
        assert_eq!(artifact.digest, digest);
        assert_eq!(artifact.encode(limits).unwrap(), bytes);
        artifact.verify(limits).unwrap();
    }
    record(&mut artifact, false, limits_for(1)).unwrap();
    artifact.verify(limits_for(1)).unwrap();
}

#[test]
fn rectangle_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let rectangles = [
        crate::BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(2, 3)).unwrap(),
        crate::BipersistenceRectangle::new(Bigrade::new(0, 0), Bigrade::new(1, 1)).unwrap(),
    ];
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_rectangles: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_rectangle(&module, rectangles[usize::from(alternate)], limits)
        },
    );
}

#[test]
fn region_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let regions = [
        BipersistenceRegion::new(vec![Bigrade::new(1, 1), Bigrade::new(1, 2)]).unwrap(),
        BipersistenceRegion::new(vec![Bigrade::new(0, 0), Bigrade::new(0, 1)]).unwrap(),
    ];
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_regions: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_region(&module, regions[usize::from(alternate)].clone(), limits)
        },
    );
}

fn class_atlases(module: &crate::BipersistenceModule) -> [crate::CohomologyClassAtlas; 2] {
    [1, 2].map(|coefficient| {
        module
            .class_atlas(
                Bigrade::new(1, 1),
                &[BipersistenceTerm {
                    basis_index: 0,
                    coefficient,
                }],
            )
            .unwrap()
    })
}

#[test]
fn class_atlas_collection_limit_is_transactional() {
    let (artifact, module) = artifact_and_module();
    let atlases = class_atlases(&module);
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_class_atlases: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_class_atlas(&module, &atlases[usize::from(alternate)], limits)
        },
    );
}

#[test]
fn circular_family_collection_limit_is_transactional() {
    let (mut artifact, module) = artifact_and_module();
    let atlases = class_atlases(&module);
    for atlas in &atlases {
        artifact
            .record_class_atlas(&module, atlas, Default::default())
            .unwrap();
    }
    assert_collection_limits(
        artifact,
        |maximum| BipersistenceArtifactLimits {
            max_circular_families: maximum,
            ..Default::default()
        },
        |artifact, alternate, limits| {
            artifact.record_circular_family(
                &module,
                &atlases[usize::from(alternate)],
                Default::default(),
                limits,
            )
        },
    );
}
