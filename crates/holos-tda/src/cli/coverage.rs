//! Relative coverage and affine coverage command workflows.

use std::path::Path;

use crate::io;
use crate::{
    CoverageAction, CoverageFence, CoverageGeometry, CoverageGeometryLimits, CoverageLimits,
    CoverageSpecification, CoverageState, CoverageSynthesisArtifact, CoverageSynthesisLimits,
    GeometryBoundCoverageArtifact, KineticFiltration, KineticLimits, PlanarCoverageModel,
    SparseDistanceMatrix,
};

use super::args::{AffineCoverageCli, CoverageCli};
use super::coverage_geometry;
use super::input::{read_kinetic_edges, write_via_temporary};

pub(super) fn run_coverage(cli: CoverageCli) -> crate::Result<()> {
    let model = PlanarCoverageModel::new(cli.broadcast_radius, cli.sensing_radius)?;
    let fence = CoverageFence::new(cli.fence.clone())?;
    let base = coverage_base(fence.vertices(), &cli.base);
    let states = read_coverage_states(&cli, base)?;
    let specification = CoverageSpecification::new(
        cli.vertices,
        model,
        cli.modulus,
        fence,
        cli.failable,
        cli.failure_budget,
        states,
        CoverageLimits::default(),
    )?;
    let actions = parse_coverage_actions(&cli.candidates, &specification)?;
    let geometry =
        coverage_geometry::read_coverage_geometry(&cli.coordinates, &specification, cli.threads)?;
    write_coverage_artifact(
        specification,
        actions,
        geometry,
        cli.max_activations,
        cli.oracle_limit,
        cli.node_limit,
        cli.max_artifact_bytes,
        &cli.output,
        "finite",
    )
}

pub(super) fn read_coverage_states(
    cli: &CoverageCli,
    base: Vec<usize>,
) -> crate::Result<Vec<CoverageState>> {
    let mut states = Vec::with_capacity(cli.states.len());
    for (step, path) in cli.states.iter().enumerate() {
        let parsed = io::read_sparse_matrix(path, cli.threads)?;
        if parsed.len() > cli.vertices {
            return Err(crate::Error::InvalidInput(format!(
                "state {} uses a vertex above --vertices {}",
                path.display(),
                cli.vertices
            )));
        }
        let graph =
            SparseDistanceMatrix::from_triplets(cli.vertices, &parsed.edges().collect::<Vec<_>>())?;
        states.push(CoverageState::new(
            0,
            step as u64,
            &graph,
            base.clone(),
            cli.broadcast_radius,
        )?);
    }
    Ok(states)
}

pub(super) fn run_affine_coverage(cli: AffineCoverageCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let model = PlanarCoverageModel::new(cli.broadcast_radius, cli.sensing_radius)?;
    let fence = CoverageFence::new(cli.fence)?;
    let base = coverage_base(fence.vertices(), &cli.base);
    let specification = CoverageSpecification::from_kinetic(
        &trajectory,
        0,
        model,
        cli.modulus,
        fence,
        cli.failable,
        cli.failure_budget,
        base,
        CoverageLimits::default(),
    )?;
    let actions = parse_coverage_actions(&cli.candidates, &specification)?;
    write_coverage_artifact(
        specification,
        actions,
        None,
        cli.max_activations,
        cli.oracle_limit,
        cli.node_limit,
        cli.max_artifact_bytes,
        &cli.output,
        "complete affine",
    )
}

pub(super) fn coverage_base(fence: &[usize], additional: &[usize]) -> Vec<usize> {
    let mut base = fence.iter().chain(additional).copied().collect::<Vec<_>>();
    base.sort_unstable();
    base.dedup();
    base
}

pub(super) fn parse_coverage_actions(
    values: &[String],
    specification: &CoverageSpecification,
) -> crate::Result<Vec<CoverageAction>> {
    if values.len() % 3 != 0 {
        return Err(crate::Error::InvalidInput(
            "--candidate requires V COST STATES".into(),
        ));
    }
    let mut actions = values
        .chunks_exact(3)
        .map(|candidate| {
            let vertex = candidate[0].parse::<usize>().map_err(|_| {
                crate::Error::InvalidInput(format!(
                    "candidate vertex {} is not an integer",
                    candidate[0]
                ))
            })?;
            let cost = candidate[1].parse::<u64>().map_err(|_| {
                crate::Error::InvalidInput(format!(
                    "candidate cost {} is not an integer",
                    candidate[1]
                ))
            })?;
            let states = if candidate[2] == "all" {
                (0..specification.states().len()).collect()
            } else {
                candidate[2]
                    .split(',')
                    .map(|value| {
                        value.parse::<usize>().map_err(|_| {
                            crate::Error::InvalidInput(format!(
                                "candidate state {value} is not an integer"
                            ))
                        })
                    })
                    .collect::<crate::Result<Vec<_>>>()?
            };
            Ok(CoverageAction::new(vertex, cost, states))
        })
        .collect::<crate::Result<Vec<_>>>()?;
    actions.sort_by_key(|action| action.vertex);
    if actions
        .windows(2)
        .any(|pair| pair[0].vertex == pair[1].vertex)
    {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats a sensor vertex".into(),
        ));
    }
    Ok(actions)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_coverage_artifact(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    geometry: Option<CoverageGeometry>,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
    max_artifact_bytes: usize,
    output: &Path,
    scope: &str,
) -> crate::Result<()> {
    let limits = CoverageSynthesisLimits {
        max_bytes: max_artifact_bytes,
        max_oracle_calls: oracle_limit,
        max_search_nodes: node_limit,
        ..CoverageSynthesisLimits::default()
    };
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)?;
    let summary = (
        artifact.specification().states().len(),
        artifact.specification().failure_budget(),
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
    );
    let geometry_bound = geometry.is_some();
    let bytes = if let Some(geometry) = geometry {
        GeometryBoundCoverageArtifact::build(
            artifact,
            geometry,
            limits,
            CoverageGeometryLimits::default(),
        )?
        .encode(
            limits,
            CoverageGeometryLimits::default(),
            crate::GeometryBoundCoverageDecodeLimits {
                max_bytes: max_artifact_bytes,
            },
        )?
    } else {
        artifact.encode(limits)?
    };
    write_via_temporary(output, &bytes)?;
    println!(
        "certified {scope} relative coverage across {} states and failure budget {}: {}, {} activations, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        summary.0,
        summary.1,
        summary.2,
        summary.3,
        summary.4,
        summary.5,
        summary.6,
        summary.7,
        bytes.len(),
    );
    if !geometry_bound {
        eprintln!(
            "physical coverage requires the declared planar domain, sensor placement, fence, and communication assumptions"
        );
    }
    Ok(())
}
