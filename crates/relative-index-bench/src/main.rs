use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CertificateLimits, IndexDeltaProof, IndexEdit, IndexParams, IndexSnapshotProof,
    InterfacePolicy, PersistenceIndex, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};
use holos_tda_check::{IndexProofState, ProofLimits};

struct Args {
    petals: usize,
    modulus: u32,
    reps: usize,
}

fn arguments() -> Result<Args, String> {
    let values: Vec<_> = std::env::args().skip(1).collect();
    let value = |name: &str, default: &str| -> String {
        values
            .windows(2)
            .find(|pair| pair[0] == name)
            .map_or(default, |pair| pair[1].as_str())
            .to_owned()
    };
    let parse = |name: &str, default: &str| -> Result<usize, String> {
        value(name, default)
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    };
    Ok(Args {
        petals: parse("--petals", "24")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn flower(petals: usize, changed: bool) -> Result<SparseDistanceMatrix, String> {
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    for offset in 0..petals {
        let vertex = 4 + offset;
        let side = offset % 4;
        let next = (side + 1) % 4;
        let value = 2.0 + offset as f64 / 10_000.0;
        let change = if changed && offset == 0 { 0.00001 } else { 0.0 };
        edges.push((side, vertex, value + change));
        edges.push((next, vertex, value));
    }
    SparseDistanceMatrix::from_triplets(4 + petals, &edges).map_err(|error| error.to_string())
}

fn index_params(policy: InterfacePolicy) -> IndexParams {
    let mut params = IndexParams::default();
    params.max_separator_width = 4;
    params.separator_search_limit = 1_000_000;
    params.leaf_vertices = 6;
    params.interface_policy = policy;
    params
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn compile(
    graph: &SparseDistanceMatrix,
    params: &RipsParams,
    policy: InterfacePolicy,
) -> Result<PersistenceIndex, String> {
    PersistenceIndex::compile(
        graph,
        params,
        index_params(policy),
        CertificateLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if args.petals < 8 || args.reps < 5 {
        return Err("petals must be at least 8, and reps must be at least 5".into());
    }
    let graph = flower(args.petals, false)?;
    let updated = flower(args.petals, true)?;
    let params = RipsParams::new(2).with_modulus(args.modulus);
    let relative = compile(&graph, &params, InterfacePolicy::Relative)?;
    let materialized = compile(&graph, &params, InterfacePolicy::Materialize)?;
    let expected = rips_persistence_sparse(&graph, &params).map_err(|error| error.to_string())?;
    if relative.diagram().bars != expected.bars || materialized.diagram().bars != expected.bars {
        return Err("an index diagram differs from complete persistence".into());
    }
    let relative_update = relative
        .transition(&updated)
        .map_err(|error| error.to_string())?;
    let materialized_update = materialized
        .transition(&updated)
        .map_err(|error| error.to_string())?;
    if relative_update.index.diagram().bars != materialized_update.index.diagram().bars {
        return Err("relative and materialized updates disagree".into());
    }
    let relative_snapshot = IndexSnapshotProof::from_index(&relative)
        .map_err(|error| error.to_string())?
        .encode()
        .map_err(|error| error.to_string())?;
    let materialized_snapshot = IndexSnapshotProof::from_index(&materialized)
        .map_err(|error| error.to_string())?
        .encode()
        .map_err(|error| error.to_string())?;
    let relative_delta = IndexDeltaProof::between(&relative, &relative_update.index)
        .map_err(|error| error.to_string())?
        .encode()
        .map_err(|error| error.to_string())?;
    let materialized_delta = IndexDeltaProof::between(&materialized, &materialized_update.index)
        .map_err(|error| error.to_string())?
        .encode()
        .map_err(|error| error.to_string())?;
    IndexProofState::verify_snapshot(&relative_snapshot, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    IndexProofState::verify_snapshot(&materialized_snapshot, ProofLimits::default())
        .map_err(|error| error.to_string())?;

    let mut relative_compile = Vec::with_capacity(args.reps);
    let mut materialized_compile = Vec::with_capacity(args.reps);
    let mut relative_update_times = Vec::with_capacity(args.reps);
    let mut materialized_update_times = Vec::with_capacity(args.reps);
    let mut relative_verify = Vec::with_capacity(args.reps);
    let mut materialized_verify = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(compile(&graph, &params, InterfacePolicy::Relative)?);
        relative_compile.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(compile(&graph, &params, InterfacePolicy::Materialize)?);
        materialized_compile.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(
            relative
                .transition_edits(&[IndexEdit::set_weight(0, 4, 2.00001)])
                .map_err(|error| error.to_string())?,
        );
        relative_update_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(
            materialized
                .transition_edits(&[IndexEdit::set_weight(0, 4, 2.00001)])
                .map_err(|error| error.to_string())?,
        );
        materialized_update_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        let (mut state, _) =
            IndexProofState::verify_snapshot(&relative_snapshot, ProofLimits::default())
                .map_err(|error| error.to_string())?;
        black_box(
            state
                .apply_delta(&relative_delta, ProofLimits::default())
                .map_err(|error| error.to_string())?,
        );
        relative_verify.push(start.elapsed().as_nanos());

        let start = Instant::now();
        let (mut state, _) =
            IndexProofState::verify_snapshot(&materialized_snapshot, ProofLimits::default())
                .map_err(|error| error.to_string())?;
        black_box(
            state
                .apply_delta(&materialized_delta, ProofLimits::default())
                .map_err(|error| error.to_string())?,
        );
        materialized_verify.push(start.elapsed().as_nanos());
    }
    let summary = relative.summary();
    println!(
        "format=holos-relative-index-bench-v1 petals={} vertices={} edges={} modulus={} reps={} nodes={} leaves={} separators={} relative_input_cells={} relative_core_cells={} relative_cancellations={} relative_compile_ns={} materialized_compile_ns={} relative_update_ns={} materialized_update_ns={} nodes_shared={} relative_nodes_rebuilt={} relative_nodes_composed={} relative_snapshot_bytes={} materialized_snapshot_bytes={} relative_delta_bytes={} materialized_delta_bytes={} relative_verify_ns={} materialized_verify_ns={} bars={}",
        args.petals,
        graph.len(),
        graph.num_edges(),
        args.modulus,
        args.reps,
        summary.nodes,
        summary.leaves,
        summary.separators,
        summary.relative_input_cells,
        summary.relative_core_cells,
        summary.relative_cancellations,
        median(relative_compile),
        median(materialized_compile),
        median(relative_update_times),
        median(materialized_update_times),
        relative_update.work.nodes_shared,
        relative_update.work.relative_nodes_rebuilt,
        relative_update.work.relative_nodes_composed,
        relative_snapshot.len(),
        materialized_snapshot.len(),
        relative_delta.len(),
        materialized_delta.len(),
        median(relative_verify),
        median(materialized_verify),
        relative.diagram().bars.len(),
    );
    Ok(())
}
