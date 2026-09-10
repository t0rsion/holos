mod configuration;
mod gates;
mod pipeline;

use super::model::{Args, Config, Counters, OrderedGate, Outcome, Samples, Verification};
use super::reporting;
use configuration::configurations;
use gates::diagrams_equal;
use pipeline::run_pipeline;

pub(super) fn run(points: &[Vec<f64>], args: &Args) -> Result<(), String> {
    let configs = configurations(args);
    let verification = verify_configurations(points, args, &configs)?;
    let hwm_start = reporting::vm_hwm_kb();
    let rotation = reporting::rotation_orders(configs.len(), args.reps);
    reporting::print_header(
        args,
        points.len(),
        &configs,
        &verification.outcomes,
        &rotation,
    );
    for line in &verification.gate_lines {
        println!("{line}");
    }
    let mut samples = collect_samples(points, args, &configs, &verification.outcomes, &rotation)?;
    reporting::print_samples(
        args,
        &configs,
        &verification.outcomes,
        &mut samples.phases,
        &samples.counters,
    );
    println!(
        "kind=memory entry={} config=all vm_hwm_kb={} vm_hwm_kb_at_start={} scope=process_high_water",
        args.entry,
        reporting::report_kb(reporting::vm_hwm_kb()),
        reporting::report_kb(hwm_start)
    );
    Ok(())
}

fn verify_configurations(
    points: &[Vec<f64>],
    args: &Args,
    configs: &[Config],
) -> Result<Verification, String> {
    let mut verified: Vec<Outcome> = Vec::with_capacity(configs.len());
    let mut gate = OrderedGate {
        serial_v1: None,
        lines: Vec::new(),
    };
    for cfg in configs {
        let mut outcome = run_pipeline(points, args, cfg, true)?;
        if let Some(first) = verified.first() {
            if !diagrams_equal(&first.diagram, &outcome.diagram) {
                return Err(format!(
                    "entry {}: configuration {} disagrees with {} bar for bar ({} bars vs {}); timings void",
                    args.entry,
                    cfg.name,
                    configs[0].name,
                    outcome.diagram.bars.len(),
                    first.diagram.bars.len()
                ));
            }
        }
        gate.observe(args, cfg, outcome.collapsed.take())?;
        verified.push(outcome);
    }
    Ok(Verification {
        outcomes: verified,
        gate_lines: gate.lines,
    })
}

fn collect_samples(
    points: &[Vec<f64>],
    args: &Args,
    configs: &[Config],
    verified: &[Outcome],
    rotation: &[Vec<usize>],
) -> Result<Samples, String> {
    let mut samples: Vec<Vec<Vec<f64>>> = verified
        .iter()
        .map(|verify| vec![Vec::with_capacity(args.reps); verify.phases.len()])
        .collect();
    // Counters and clocks come from the timed repetitions, never from the
    // agreement run: that run's schedule is the same, but its clocks are one
    // cold sample.
    let mut counter_runs: Vec<Vec<Counters>> = configs
        .iter()
        .map(|_| Vec::with_capacity(args.reps))
        .collect();
    for (rep, order) in rotation.iter().enumerate() {
        for &index in order {
            let cfg = &configs[index];
            let mut outcome = run_pipeline(points, args, cfg, false)?;
            // After the clocks stop: a mid-run diagram change is a hard
            // failure, not a timing.
            if !diagrams_equal(&outcome.diagram, &verified[index].diagram) {
                return Err(format!(
                    "config {} rep {rep}: diagram differs from the agreement run",
                    cfg.name
                ));
            }
            for (slot, (_, seconds)) in samples[index].iter_mut().zip(&outcome.phases) {
                slot.push(*seconds);
            }
            if let Some(counters) = outcome.counters.take() {
                counter_runs[index].push(counters);
            }
        }
    }
    Ok(Samples {
        phases: samples,
        counters: counter_runs,
    })
}
