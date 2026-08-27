#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use holos_tda_check::{
    IndexProofState, ProofBundle, ProofLimits, VerifiedCoverageSource, VerifiedSynthesisSource,
    is_cohomology_intervention, is_coverage, is_distributed_interface, is_explicit_persistence,
    is_index_snapshot, is_kinetic_zigzag, is_relative_interface, is_synthesis,
    verify_cohomology_intervention, verify_coverage, verify_distributed_interface_with,
    verify_explicit_persistence, verify_kinetic_zigzag, verify_relative_interface,
    verify_synthesis,
};
use sha2::Digest;

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

fn artifact_probes() -> [(ArtifactProbe, ArtifactKind); 8] {
    [
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

fn artifact_runners() -> [(ArtifactKind, ArtifactRunner); 9] {
    [
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

fn run_distributed(bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    let paths = distributed_paths(rest)?;
    let checked = verify_distributed_interface_with(
        bytes,
        |id| {
            let path = paths
                .get(id)
                .ok_or_else(|| holos_tda_check::ProofError::new("distributed object is absent"))?;
            fs::read(path).map_err(|error| {
                holos_tda_check::ProofError::new(format!("read {:?}: {error}", path))
            })
        },
        ProofLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    if checked.objects != paths.len() {
        return Err("distributed object set differs from the manifest references".into());
    }
    println!(
        "verified distributed interface through dimension {} with {} shards, {} composition folds, and {} unique objects",
        checked.max_dim, checked.shards, checked.folds, checked.objects,
    );
    Ok(())
}

fn distributed_paths(
    rest: &[std::ffi::OsString],
) -> Result<BTreeMap<[u8; 32], std::ffi::OsString>, String> {
    let mut paths = BTreeMap::new();
    for path in rest {
        let object = fs::read(path).map_err(|error| format!("read {:?}: {error}", path))?;
        let id: [u8; 32] = sha2::Sha256::digest(&object).into();
        if paths.insert(id, path.clone()).is_some() {
            return Err("distributed object list repeats an artifact".into());
        }
    }
    Ok(paths)
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

fn run_index(bytes: &[u8], rest: &[std::ffi::OsString]) -> Result<(), String> {
    let (mut state, cold) = IndexProofState::verify_snapshot(bytes, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    let mut counts = IndexCounts {
        checkpoints: 1,
        higher_columns: cold.higher_columns_checked,
        ..IndexCounts::default()
    };
    for path in rest {
        apply_index_record(&mut state, &mut counts, path)?;
    }
    println!(
        "verified persistence through dimension {} across {} index checkpoints and {} later records with {} initial interfaces, {} changed interfaces, {} edge changes, and {} higher boundary columns",
        state.max_dim(),
        counts.checkpoints,
        counts.records,
        cold.nodes_checked,
        counts.changed_nodes,
        counts.edge_changes,
        counts.higher_columns,
    );
    Ok(())
}

#[derive(Default)]
struct IndexCounts {
    records: usize,
    checkpoints: usize,
    changed_nodes: usize,
    edge_changes: usize,
    higher_columns: usize,
}

fn apply_index_record(
    state: &mut IndexProofState,
    counts: &mut IndexCounts,
    path: &std::ffi::OsString,
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("read {:?}: {error}", path))?;
    if is_index_snapshot(&bytes) {
        let (next, checked) = IndexProofState::verify_snapshot(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        *state = next;
        counts.checkpoints += 1;
        counts.changed_nodes += checked.nodes_checked;
        counts.higher_columns += checked.higher_columns_checked;
    } else {
        let checked = state
            .apply_delta(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        counts.changed_nodes += checked.nodes_checked;
        counts.edge_changes += checked.edge_changes;
        counts.higher_columns += checked.higher_columns_checked;
    }
    counts.records += 1;
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
