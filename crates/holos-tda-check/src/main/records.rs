use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;

use holos_tda_check::{
    IndexProofState, ProofLimits, is_index_snapshot, verify_distributed_interface_with,
};
use sha2::Digest;

pub(super) fn run_distributed(bytes: &[u8], rest: &[OsString]) -> Result<(), String> {
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

fn distributed_paths(rest: &[OsString]) -> Result<BTreeMap<[u8; 32], OsString>, String> {
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

pub(super) fn run_index(bytes: &[u8], rest: &[OsString]) -> Result<(), String> {
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
    path: &OsString,
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
