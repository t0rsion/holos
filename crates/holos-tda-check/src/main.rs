#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use holos_tda_check::{
    BipersistenceProofLimits, CircularProofLimits, ProgramGraph, ProgramProofLimits,
    ProgramTraceProofLimits, ProofBundle, ProofLimits, VerifiedCoverageSource,
    VerifiedSynthesisSource, is_bipersistence, is_circular_coordinate, is_cohomology_intervention,
    is_coverage, is_distributed_interface, is_explicit_persistence, is_geometry_bound_coverage,
    is_index_snapshot, is_kinetic_zigzag, is_program, is_program_trace, is_relative_interface,
    is_synthesis, verify_bipersistence, verify_circular_coordinate, verify_cohomology_intervention,
    verify_coverage, verify_explicit_persistence, verify_geometry_bound_coverage,
    verify_kinetic_zigzag, verify_program, verify_program_trace, verify_relative_interface,
    verify_synthesis,
};

#[path = "main/records.rs"]
mod records;

use records::{run_distributed, run_index};

fn run() -> Result<(), String> {
    let mut arguments = env::args_os();
    let program = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "holos-check".into());
    let Some(path) = arguments.next() else {
        return Err(format!("usage: {program} SNAPSHOT [RECORD ...]"));
    };
    let bytes = fs::read(&path).map_err(|error| format!("read {:?}: {error}", path))?;
    let rest = arguments.collect::<Vec<_>>();
    run_artifact(artifact_kind(&bytes), &program, &bytes, &rest)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Program,
    ProgramTrace,
    Bipersistence,
    Circular,
    GeometryBoundCoverage,
    Coverage,
    Synthesis,
    KineticZigzag,
    Intervention,
    Distributed,
    Explicit,
    Relative,
    Index,
    Trajectory,
}

fn artifact_kind(bytes: &[u8]) -> ArtifactKind {
    artifact_probes()
        .iter()
        .find_map(|(probe, kind)| probe(bytes).then_some(*kind))
        .unwrap_or(ArtifactKind::Trajectory)
}

type ArtifactProbe = fn(&[u8]) -> bool;
type ArtifactRunner = fn(&str, &[u8], &[std::ffi::OsString]) -> Result<(), String>;

fn artifact_probes() -> [(ArtifactProbe, ArtifactKind); 13] {
    [
        (is_program_trace, ArtifactKind::ProgramTrace),
        (is_program, ArtifactKind::Program),
        (is_bipersistence, ArtifactKind::Bipersistence),
        (is_circular_coordinate, ArtifactKind::Circular),
        (
            is_geometry_bound_coverage,
            ArtifactKind::GeometryBoundCoverage,
        ),
        (is_coverage, ArtifactKind::Coverage),
        (is_synthesis, ArtifactKind::Synthesis),
        (is_kinetic_zigzag, ArtifactKind::KineticZigzag),
        (is_cohomology_intervention, ArtifactKind::Intervention),
        (is_distributed_interface, ArtifactKind::Distributed),
        (is_explicit_persistence, ArtifactKind::Explicit),
        (is_relative_interface, ArtifactKind::Relative),
        (is_index_snapshot, ArtifactKind::Index),
    ]
}

fn run_artifact(
    kind: ArtifactKind,
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    let runner = artifact_runners()
        .iter()
        .find_map(|(candidate, runner)| (*candidate == kind).then_some(*runner))
        .expect("every artifact kind has a runner");
    runner(program, bytes, rest)
}

fn artifact_runners() -> [(ArtifactKind, ArtifactRunner); 14] {
    [
        (ArtifactKind::ProgramTrace, run_program_trace),
        (ArtifactKind::Program, run_program),
        (ArtifactKind::Bipersistence, run_bipersistence),
        (ArtifactKind::Circular, run_circular),
        (
            ArtifactKind::GeometryBoundCoverage,
            run_geometry_bound_coverage,
        ),
        (ArtifactKind::Coverage, run_coverage),
        (ArtifactKind::Synthesis, run_synthesis),
        (ArtifactKind::KineticZigzag, run_kinetic_zigzag),
        (ArtifactKind::Intervention, run_intervention),
        (ArtifactKind::Distributed, run_distributed_adapter),
        (ArtifactKind::Explicit, run_explicit),
        (ArtifactKind::Relative, run_relative),
        (ArtifactKind::Index, run_index_adapter),
        (ArtifactKind::Trajectory, run_trajectory),
    ]
}

fn run_program_trace(
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a persistence program trace: {program} TRACE"),
    )?;
    let checked = verify_program_trace(bytes, ProgramTraceProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified H0 and H1 persistence trace over Z/{} on {} vertices and {} edges across {} steps with {} final bars; graph bindings, reduction replay, work counters, continuations, and correspondences checked, scheduling policy not certified",
        checked.modulus, checked.vertices, checked.edges, checked.steps, checked.bars,
    );
    Ok(())
}

fn run_program(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    if rest.len() != 1 {
        return Err(format!(
            "usage for a persistence program: {program} PROGRAM SOURCE_GRAPH"
        ));
    }
    let graph_bytes = fs::read(&rest[0]).map_err(|error| format!("read {:?}: {error}", rest[0]))?;
    let graph = ProgramGraph::parse_text(&graph_bytes).map_err(|error| error.to_string())?;
    let checked = verify_program(bytes, &graph, ProgramProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified H0 and H1 persistence program over Z/{} on {} vertices and {} edges with {} atoms, {} cyclic atoms, {} class spaces, {} reduction columns, and {} bars",
        checked.modulus,
        checked.vertices,
        checked.edges,
        checked.atoms,
        checked.cyclic_atoms,
        checked.class_spaces,
        checked.reduction_columns,
        checked.bars,
    );
    Ok(())
}

fn run_bipersistence(
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    require_single_artifact(rest, format!("usage for bipersistence: {program} ARTIFACT"))?;
    let checked = verify_bipersistence(bytes, BipersistenceProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified degree-Rips H1 bipersistence over Z/{} on {} vertices, {} edges, a {} by {} grid, {} cover maps, {} rectangle claims, {} connected-region claims, {} class atlases, and {} circular families",
        checked.modulus,
        checked.vertices,
        checked.edges,
        checked.scales,
        checked.density_levels,
        checked.cover_maps,
        checked.rectangles,
        checked.regions,
        checked.class_atlases,
        checked.circular_families,
    );
    Ok(())
}

fn run_circular(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a circular coordinate: {program} ARTIFACT"),
    )?;
    let checked = verify_circular_coordinate(bytes, CircularProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified {} circular coordinates over Z/{} on {} states and {} active edges at scale {} with maximum relative residual {}, divisibilities {:?}, and continuation {:?}",
        checked.coordinates,
        checked.modulus,
        checked.states,
        checked.edges,
        checked.scale,
        checked.max_relative_residual,
        checked.divisibilities,
        checked.continuation,
    );
    Ok(())
}

fn run_geometry_bound_coverage(
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for geometry-bound coverage: {program} ARTIFACT"),
    )?;
    let checked = verify_geometry_bound_coverage(bytes, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified planar coverage over Z/{} across {} states and {} sensors with {} exact pair checks, {:?} status, {} selected actions, and cost bounds {:?} to {:?}",
        checked.coverage.modulus,
        checked.states,
        checked.vertices,
        checked.pair_checks,
        checked.coverage.status,
        checked.coverage.selected,
        checked.coverage.lower_bound_cost,
        checked.coverage.upper_bound_cost,
    );
    Ok(())
}

fn run_distributed_adapter(
    _program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    run_distributed(bytes, rest)
}

fn run_index_adapter(
    _program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    run_index(bytes, rest)
}

fn require_single_artifact(rest: &[std::ffi::OsString], usage: String) -> Result<(), String> {
    if rest.is_empty() { Ok(()) } else { Err(usage) }
}

fn run_coverage(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a coverage proof: {program} ARTIFACT"),
    )?;
    let checked =
        verify_coverage(bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    let scope = match checked.source {
        VerifiedCoverageSource::Finite => "listed-state",
        VerifiedCoverageSource::Affine => "complete affine",
    };
    println!(
        "verified {scope} relative coverage over Z/{} across {} states, {} actions, and failure budget {} with {:?} status, {} selected actions, cost bounds {:?} to {:?}, {} selected-plan failure checks, and {} proof topology checks",
        checked.modulus,
        checked.states,
        checked.actions,
        checked.failure_budget,
        checked.status,
        checked.selected,
        checked.lower_bound_cost,
        checked.upper_bound_cost,
        checked.selected_failure_checks,
        checked.proof_topology_checks,
    );
    Ok(())
}

fn run_synthesis(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a synthesis proof: {program} ARTIFACT"),
    )?;
    let checked =
        verify_synthesis(bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    let scope = match checked.source {
        VerifiedSynthesisSource::Finite => "listed-state",
        VerifiedSynthesisSource::Affine => "complete affine",
    };
    println!(
        "verified H{} {scope} synthesis over Z/{} across {} states and {} actions with {:?} status, {} selected actions, cost bounds {:?} to {:?}, {} producer topology calls, and {} proof topology checks",
        checked.dimension,
        checked.modulus,
        checked.states,
        checked.actions,
        checked.status,
        checked.selected,
        checked.lower_bound_cost,
        checked.upper_bound_cost,
        checked.producer_oracle_calls,
        checked.proof_topology_checks,
    );
    Ok(())
}

fn run_kinetic_zigzag(
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a kinetic zigzag: {program} ARTIFACT"),
    )?;
    let checked =
        verify_kinetic_zigzag(bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    println!(
        "verified H{} kinetic zigzag over Z/{} with {} nodes, {} arrows, {} interval spaces, and {} interval copies",
        checked.dimension,
        checked.modulus,
        checked.nodes,
        checked.arrows,
        checked.intervals,
        checked.interval_copies,
    );
    Ok(())
}

fn run_intervention(
    program: &str,
    bytes: &[u8],
    rest: &[std::ffi::OsString],
) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a cohomology intervention: {program} ARTIFACT"),
    )?;
    let checked = verify_cohomology_intervention(bytes, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified H{} intervention across {} scenarios with {:?} status, {} edits, {} oracle calls, and cost bounds {:?} to {:?}",
        checked.dimension,
        checked.scenarios,
        checked.status,
        checked.edits,
        checked.oracle_calls,
        checked.lower_bound_cost,
        checked.upper_bound_cost,
    );
    Ok(())
}

fn run_relative(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a relative interface: {program} CERTIFICATE"),
    )?;
    let checked = verify_relative_interface(bytes, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified relative interface through dimension {} with {} cancellations, {} retained cells, {} reduction columns, and {} bars",
        checked.max_dim,
        checked.cancellations,
        checked.core_cells,
        checked.reduction_columns,
        checked.bars,
    );
    Ok(())
}

fn run_explicit(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for an explicit persistence proof: {program} CERTIFICATE"),
    )?;
    let checked = verify_explicit_persistence(bytes, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    println!(
        "verified explicit persistence through dimension {} over Z/{} with {} vertices, {} simplices, {} change columns, {} change terms, and {} bars",
        checked.max_homology_dimension,
        checked.modulus,
        checked.vertices,
        checked.simplex_counts.iter().sum::<usize>(),
        checked.change_columns,
        checked.change_terms,
        checked.bars.len(),
    );
    Ok(())
}

fn run_trajectory(program: &str, bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    require_single_artifact(
        rest,
        format!("usage for a trajectory proof: {program} PROOF"),
    )?;
    let proof =
        ProofBundle::decode(bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    let verified = proof.verify().map_err(|error| error.to_string())?;
    println!(
        "verified {} snapshots through {} unique reduction nodes; {} references reused, {} weighted reductions cached",
        verified.snapshots,
        verified.unique_nodes,
        verified.reused_references,
        verified.cached_references
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
