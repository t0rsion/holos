#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use holos_tda_check::{
    IndexProofState, ProofBundle, ProofLimits, VerifiedCoverageSource, VerifiedSynthesisSource,
    is_cohomology_intervention, is_coverage, is_distributed_interface, is_index_snapshot,
    is_kinetic_zigzag, is_relative_interface, is_synthesis, verify_cohomology_intervention,
    verify_coverage, verify_distributed_interface_with, verify_kinetic_zigzag,
    verify_relative_interface, verify_synthesis,
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
    if is_coverage(&bytes) {
        if arguments.next().is_some() {
            return Err(format!("usage for a coverage proof: {program} ARTIFACT"));
        }
        let checked =
            verify_coverage(&bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
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
    } else if is_synthesis(&bytes) {
        if arguments.next().is_some() {
            return Err(format!("usage for a synthesis proof: {program} ARTIFACT"));
        }
        let checked =
            verify_synthesis(&bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
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
    } else if is_kinetic_zigzag(&bytes) {
        if arguments.next().is_some() {
            return Err(format!("usage for a kinetic zigzag: {program} ARTIFACT"));
        }
        let checked = verify_kinetic_zigzag(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        println!(
            "verified H{} kinetic zigzag over Z/{} with {} nodes, {} arrows, {} interval spaces, and {} interval copies",
            checked.dimension,
            checked.modulus,
            checked.nodes,
            checked.arrows,
            checked.intervals,
            checked.interval_copies,
        );
    } else if is_cohomology_intervention(&bytes) {
        if arguments.next().is_some() {
            return Err(format!(
                "usage for a cohomology intervention: {program} ARTIFACT"
            ));
        }
        let checked = verify_cohomology_intervention(&bytes, ProofLimits::default())
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
    } else if is_distributed_interface(&bytes) {
        let mut paths = BTreeMap::new();
        for path in arguments {
            let object = fs::read(&path).map_err(|error| format!("read {:?}: {error}", path))?;
            let id: [u8; 32] = sha2::Sha256::digest(&object).into();
            if paths.insert(id, path).is_some() {
                return Err("distributed object list repeats an artifact".into());
            }
        }
        let checked = verify_distributed_interface_with(
            &bytes,
            |id| {
                let path = paths.get(id).ok_or_else(|| {
                    holos_tda_check::ProofError::new("distributed object is absent")
                })?;
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
    } else if is_relative_interface(&bytes) {
        if arguments.next().is_some() {
            return Err(format!(
                "usage for a relative interface: {program} CERTIFICATE"
            ));
        }
        let checked = verify_relative_interface(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        println!(
            "verified relative interface through dimension {} with {} cancellations, {} retained cells, {} reduction columns, and {} bars",
            checked.max_dim,
            checked.cancellations,
            checked.core_cells,
            checked.reduction_columns,
            checked.bars,
        );
    } else if is_index_snapshot(&bytes) {
        let (mut state, cold) = IndexProofState::verify_snapshot(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        let mut records = 0usize;
        let mut checkpoints = 1usize;
        let mut changed_nodes = 0usize;
        let mut edge_changes = 0usize;
        let mut higher_columns = cold.higher_columns_checked;
        for path in arguments {
            let bytes = fs::read(&path).map_err(|error| format!("read {:?}: {error}", path))?;
            if is_index_snapshot(&bytes) {
                let (next, checked) =
                    IndexProofState::verify_snapshot(&bytes, ProofLimits::default())
                        .map_err(|error| error.to_string())?;
                state = next;
                checkpoints += 1;
                changed_nodes += checked.nodes_checked;
                higher_columns += checked.higher_columns_checked;
            } else {
                let checked = state
                    .apply_delta(&bytes, ProofLimits::default())
                    .map_err(|error| error.to_string())?;
                changed_nodes += checked.nodes_checked;
                edge_changes += checked.edge_changes;
                higher_columns += checked.higher_columns_checked;
            }
            records += 1;
        }
        println!(
            "verified persistence through dimension {} across {checkpoints} index checkpoints and {records} later records with {} initial interfaces, {changed_nodes} changed interfaces, {edge_changes} edge changes, and {higher_columns} higher boundary columns",
            state.max_dim(),
            cold.nodes_checked,
        );
    } else {
        if arguments.next().is_some() {
            return Err(format!("usage for a trajectory proof: {program} PROOF"));
        }
        let proof = ProofBundle::decode(&bytes, ProofLimits::default())
            .map_err(|error| error.to_string())?;
        let verified = proof.verify().map_err(|error| error.to_string())?;
        println!(
            "verified {} snapshots through {} unique reduction nodes; {} references reused, {} weighted reductions cached",
            verified.snapshots,
            verified.unique_nodes,
            verified.reused_references,
            verified.cached_references
        );
    }
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
