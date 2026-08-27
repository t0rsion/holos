use std::time::Instant;

use holos_tda::{
    CoverageAction, CoverageFence, CoverageGeometry, CoverageGeometryLimits, CoverageLimits,
    CoverageSpecification, CoverageState, CoverageSynthesisArtifact, CoverageSynthesisLimits,
    GeometryBoundCoverageArtifact, GeometryBoundCoverageDecodeLimits, PlanarCoverageModel,
    PlanarPoint, SparseDistanceMatrix,
};
use holos_tda_check::{ProofLimits, verify_geometry_bound_coverage};

use crate::{Measurement, median};

pub(crate) fn measure(repetitions: usize) -> Result<Measurement, String> {
    let (specification, geometry, action) = specimen()?;
    let coverage_limits = CoverageSynthesisLimits::default();
    let geometry_limits = CoverageGeometryLimits::default();
    let mut producer_times = Vec::with_capacity(repetitions);
    let mut checker_times = Vec::with_capacity(repetitions);
    let mut final_bytes = Vec::new();
    let mut pair_checks = 0;
    for _ in 0..repetitions {
        let started = Instant::now();
        let coverage = CoverageSynthesisArtifact::build(
            specification.clone(),
            vec![action.clone()],
            1,
            coverage_limits,
        )
        .map_err(|error| error.to_string())?;
        let artifact = GeometryBoundCoverageArtifact::build(
            coverage,
            geometry.clone(),
            coverage_limits,
            geometry_limits,
        )
        .map_err(|error| error.to_string())?;
        let bytes = artifact
            .encode(
                coverage_limits,
                geometry_limits,
                GeometryBoundCoverageDecodeLimits::default(),
            )
            .map_err(|error| error.to_string())?;
        producer_times.push(started.elapsed());
        let started = Instant::now();
        let checked = verify_geometry_bound_coverage(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        checker_times.push(started.elapsed());
        pair_checks = checked.pair_checks;
        final_bytes = bytes;
    }
    Ok(Measurement {
        family: "geometry",
        case: "square-center",
        producer: median(producer_times),
        checker: median(checker_times),
        artifact_bytes: final_bytes.len(),
        work: format!("states=1 vertices=5 pair_checks={pair_checks} checker=independent"),
    })
}

fn specimen() -> Result<(CoverageSpecification, CoverageGeometry, CoverageAction), String> {
    let points = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (1.0, 1.0)]
        .into_iter()
        .map(|(x, y)| PlanarPoint::new(x, y).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let root_two = 2f64.sqrt();
    let graph = SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 2.0),
            (1, 2, 2.0),
            (2, 3, 2.0),
            (0, 3, 2.0),
            (0, 4, root_two),
            (1, 4, root_two),
            (2, 4, root_two),
            (3, 4, root_two),
        ],
    )
    .map_err(|error| error.to_string())?;
    let specification = CoverageSpecification::new(
        5,
        PlanarCoverageModel::new(2.0, 2.0).map_err(|error| error.to_string())?,
        2,
        CoverageFence::new(vec![0, 1, 2, 3]).map_err(|error| error.to_string())?,
        Vec::new(),
        0,
        vec![
            CoverageState::new(0, 0, &graph, (0..4).collect(), 2.0)
                .map_err(|error| error.to_string())?,
        ],
        CoverageLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let geometry = CoverageGeometry::new(
        &specification,
        vec![points],
        CoverageGeometryLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let action = CoverageAction::throughout(4, 1, &specification);
    Ok((specification, geometry, action))
}
