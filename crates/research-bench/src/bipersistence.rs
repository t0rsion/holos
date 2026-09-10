use std::time::Instant;

use holos_tda::{
    Bigrade, BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceModule,
    BipersistenceRectangle, BipersistenceRegion, BipersistenceTerm, CircularCoordinateParams,
    DegreeRipsBifiltration, DegreeRipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{BipersistenceProofLimits, verify_bipersistence};

use crate::{Measurement, median};

pub(crate) fn measure(repetitions: usize) -> Result<Measurement, String> {
    let graph = specimen()?;
    let degree_rips = DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default())
        .map_err(|error| error.to_string())?;
    let limits = BipersistenceArtifactLimits::default();
    let mut producer_times = Vec::with_capacity(repetitions);
    let mut checker_times = Vec::with_capacity(repetitions);
    let mut final_bytes = Vec::new();
    let mut final_nodes = 0usize;
    let mut final_covers = 0usize;
    for _ in 0..repetitions {
        let started = Instant::now();
        let bytes = produce(&degree_rips, limits)?;
        producer_times.push(started.elapsed());
        let started = Instant::now();
        let checked = check(&bytes)?;
        checker_times.push(started.elapsed());
        final_nodes = checked.nodes;
        final_covers = checked.cover_maps;
        final_bytes = bytes;
    }
    Ok(Measurement {
        family: "bipersistence",
        case: "two-cycle-class-atlas",
        producer: median(producer_times),
        checker: median(checker_times),
        artifact_bytes: final_bytes.len(),
        work: format!(
            "nodes={final_nodes} cover_maps={final_covers} rectangles=1 regions=1 class_atlases=1 circular_families=1 checker=independent"
        ),
    })
}

fn produce(
    degree_rips: &DegreeRipsBifiltration,
    limits: BipersistenceArtifactLimits,
) -> Result<Vec<u8>, String> {
    let (mut artifact, module) =
        BipersistenceArtifact::build(degree_rips, 47, limits).map_err(|error| error.to_string())?;
    record_rank_claims(&mut artifact, &module, limits)?;
    record_class_claims(&mut artifact, &module, limits)?;
    artifact.encode(limits).map_err(|error| error.to_string())
}

fn record_rank_claims(
    artifact: &mut BipersistenceArtifact,
    module: &BipersistenceModule,
    limits: BipersistenceArtifactLimits,
) -> Result<(), String> {
    let rectangle = BipersistenceRectangle::new(Bigrade::new(1, 6), Bigrade::new(2, 6))
        .map_err(|error| error.to_string())?;
    artifact
        .record_rectangle(module, rectangle, limits)
        .map_err(|error| error.to_string())?;
    let region = BipersistenceRegion::new(vec![
        Bigrade::new(0, 6),
        Bigrade::new(1, 5),
        Bigrade::new(1, 6),
        Bigrade::new(2, 5),
    ])
    .map_err(|error| error.to_string())?;
    artifact
        .record_region(module, region, limits)
        .map_err(|error| error.to_string())
}

fn record_class_claims(
    artifact: &mut BipersistenceArtifact,
    module: &BipersistenceModule,
    limits: BipersistenceArtifactLimits,
) -> Result<(), String> {
    let atlas = module
        .class_atlas(
            Bigrade::new(1, 6),
            &[BipersistenceTerm {
                basis_index: 0,
                coefficient: 1,
            }],
        )
        .map_err(|error| error.to_string())?;
    artifact
        .record_class_atlas(module, &atlas, limits)
        .map_err(|error| error.to_string())?;
    artifact
        .record_circular_family(module, &atlas, CircularCoordinateParams::default(), limits)
        .map_err(|error| error.to_string())
}

fn check(bytes: &[u8]) -> Result<holos_tda_check::VerifiedBipersistence, String> {
    verify_bipersistence(bytes, BipersistenceProofLimits::default())
        .map_err(|error| error.to_string())
}

fn specimen() -> Result<SparseDistanceMatrix, String> {
    SparseDistanceMatrix::from_triplets(
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
    .map_err(|error| error.to_string())
}
