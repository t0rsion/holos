//! Stable machine-readable record.

use super::execution::StudyResult;

pub(crate) fn line(result: &StudyResult) -> String {
    let compile_ns = median(&result.compile_times);
    let update_ns = median(&result.update_times);
    let diagram_ns = median(&result.diagram_times);
    let program_verify_ns = median(&result.program_verify_times);
    let trace_verify_ns = median(&result.trace_verify_times);
    format!(
        "format=holos-public-program-bench-v1 dataset={} source_sha256={} vertices={} edges={} snapshots={} transitions={} bin_seconds={} warmup_bins={} decay_numerator={} decay_denominator={} score_scale={} weight_offset={} maximum_score={} modulus={} initial_atoms={} initial_cyclic_atoms={} initial_complete_guards={} initial_guards={} final_complete_guards={} final_guards={} widest_separator={} order_change_steps={} accepted_steps={} accepted_fraction={:.6} strict_acceptances={} strict_fraction={:.6} reused_steps={} repaired_steps={} recompiled_steps={} guard_failure_events={} atoms_touched={} atoms_reused={} atoms_repaired={} atoms_rebuilt={} reduction_columns_reused={} reduction_columns_reduced={} reduction_column_additions={} compile_ns={} update_ns={} diagram_ns={} update_speedup={:.6} break_even_trajectories={} program_bytes={} trace_bytes={} program_verify_ns={} trace_verify_ns={} compile_samples_ns={} update_samples_ns={} diagram_samples_ns={} program_verify_samples_ns={} trace_verify_samples_ns={}",
        result.metadata.dataset,
        result.metadata.source_sha256,
        result.metadata.vertices,
        result.metadata.edges,
        result.metadata.snapshots,
        result.transitions,
        result.metadata.bin_seconds,
        result.metadata.warmup_bins,
        result.metadata.decay_numerator,
        result.metadata.decay_denominator,
        result.metadata.score_scale,
        result.metadata.weight_offset,
        result.metadata.maximum_score,
        result.modulus,
        result.initial_atoms,
        result.initial_cyclic_atoms,
        result.initial_complete_guards,
        result.initial_guards,
        result.final_complete_guards,
        result.final_guards,
        result.widest_separator,
        result.order_change_steps,
        result.accepted_steps,
        ratio(result.accepted_steps as u128, result.transitions as u128),
        result.strict_acceptances,
        ratio(
            result.strict_acceptances as u128,
            result.transitions as u128
        ),
        result.reused_steps,
        result.repaired_steps,
        result.recompiled_steps,
        result.guard_failure_events,
        result.atoms_touched,
        result.atoms_reused,
        result.atoms_repaired,
        result.atoms_rebuilt,
        result.reduction_columns_reused,
        result.reduction_columns_reduced,
        result.reduction_column_additions,
        compile_ns,
        update_ns,
        diagram_ns,
        ratio(diagram_ns, update_ns),
        break_even(compile_ns, update_ns, diagram_ns),
        result.program_bytes,
        result.trace_bytes,
        program_verify_ns,
        trace_verify_ns,
        samples(&result.compile_times),
        samples(&result.update_times),
        samples(&result.diagram_times),
        samples(&result.program_verify_times),
        samples(&result.trace_verify_times),
    )
}

fn median(values: &[u128]) -> u128 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    ordered[ordered.len() / 2]
}

fn ratio(numerator: u128, denominator: u128) -> f64 {
    numerator as f64 / denominator as f64
}

fn break_even(compile: u128, update: u128, diagram: u128) -> u128 {
    let saved = diagram.saturating_sub(update);
    if saved == 0 {
        u128::MAX
    } else {
        compile.div_ceil(saved)
    }
}

fn samples(values: &[u128]) -> String {
    values
        .iter()
        .map(u128::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
