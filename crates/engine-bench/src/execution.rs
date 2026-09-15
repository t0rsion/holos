use std::time::Instant;

use holos_tda::{
    Diagram, DistanceMatrix, Engine as LibEngine, RipsParams, SparseDistanceMatrix,
    rips_persistence, rips_persistence_sparse,
};

use super::input;
use super::model::{Args, Config, Engine, Outcome, Parsed};
use super::reporting;

pub(super) fn run(args: &Args) -> Result<(), String> {
    let configs = configurations(args);
    let verified = verify_configurations(args, &configs)?;
    let reference = verified
        .first()
        .ok_or_else(|| "no configuration ran; --mode selected none".to_string())?;
    if let Some(path) = &args.diagram_out {
        input::write_reference_diagram(path, &reference.diagram, args.max_dim)?;
    }

    let hwm_start = reporting::vm_hwm_kb();
    let rotation = reporting::rotation_orders(configs.len(), args.reps);
    reporting::print_header(args, &configs, &verified, &rotation);
    let mut samples = collect_samples(args, &configs, &verified, &rotation)?;
    reporting::print_phase_summaries(args, &configs, &verified, &mut samples);
    println!(
        "kind=memory entry={} config=all vm_hwm_kb={} vm_hwm_kb_at_start={} scope=process_high_water",
        args.entry,
        reporting::report_kb(reporting::vm_hwm_kb()),
        reporting::report_kb(hwm_start)
    );
    Ok(())
}

fn verify_configurations(args: &Args, configs: &[Config]) -> Result<Vec<Outcome>, String> {
    let mut verified: Vec<Outcome> = Vec::with_capacity(configs.len());
    for cfg in configs {
        let outcome = run_pipeline(args, cfg)?;
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
        verified.push(outcome);
    }
    Ok(verified)
}

fn collect_samples(
    args: &Args,
    configs: &[Config],
    verified: &[Outcome],
    rotation: &[Vec<usize>],
) -> Result<Vec<Vec<Vec<f64>>>, String> {
    let mut samples: Vec<Vec<Vec<f64>>> = verified
        .iter()
        .map(|verify| vec![Vec::with_capacity(args.reps); verify.phases.len()])
        .collect();
    for (rep, order) in rotation.iter().enumerate() {
        for &index in order {
            let cfg = &configs[index];
            let outcome = run_pipeline(args, cfg)?;
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
        }
    }
    Ok(samples)
}

/// One full pipeline run of one configuration, phase by phase.
fn run_pipeline(args: &Args, cfg: &Config) -> Result<Outcome, String> {
    let mut phases: Vec<(&'static str, f64)> = Vec::with_capacity(5);
    let whole = Instant::now();
    let values = timed(&mut phases, "parse", || {
        input::read_input(&args.input, args.format, args.parse_threads)
    })?;
    let params = pipeline_params(args, cfg.engine);
    let (points, mut diagram, graph_edges) = match (values, cfg.engine) {
        (Parsed::Triplets(n, triplets), Engine::Sparse) => {
            reduce_sparse_triplets(n, &triplets, &params, &mut phases)?
        }
        (Parsed::Triplets(n, triplets), Engine::Dense | Engine::Auto) => {
            reduce_widened_triplets(n, &triplets, &params, &mut phases)?
        }
        (values, engine) => reduce_dense_input(values, engine, args, &params, &mut phases)?,
    };
    phases.push(("total", whole.elapsed().as_secs_f64()));
    if points < 2 {
        return Err(format!(
            "{}: need at least two points",
            input::file_stem(&args.input)
        ));
    }
    diagram.canonicalize();

    Ok(Outcome {
        phases,
        diagram,
        points,
        graph_edges,
    })
}

fn timed<T>(
    phases: &mut Vec<(&'static str, f64)>,
    name: &'static str,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let clock = Instant::now();
    let value = operation()?;
    phases.push((name, clock.elapsed().as_secs_f64()));
    Ok(value)
}

fn pipeline_params(args: &Args, engine: Engine) -> RipsParams {
    RipsParams::new(args.max_dim)
        .with_threshold(args.threshold)
        .with_modulus(args.modulus)
        .with_threads(args.threads)
        .with_engine(match engine {
            Engine::Auto | Engine::Sparse => LibEngine::Auto,
            Engine::Dense => LibEngine::Dense,
        })
        .with_dense_storage(args.dense_storage)
}

fn reduce_sparse_triplets(
    points: usize,
    triplets: &[(usize, usize, f64)],
    params: &RipsParams,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<(usize, Diagram, Option<usize>), String> {
    let sparse = timed(phases, "graph", || {
        SparseDistanceMatrix::from_triplets(points, triplets).map_err(|error| error.to_string())
    })?;
    let edges = sparse.num_edges();
    let diagram = timed(phases, "reduce", || {
        rips_persistence_sparse(&sparse, params).map_err(|error| error.to_string())
    })?;
    Ok((points, diagram, Some(edges)))
}

fn reduce_widened_triplets(
    points: usize,
    triplets: &[(usize, usize, f64)],
    params: &RipsParams,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<(usize, Diagram, Option<usize>), String> {
    let distances = timed(phases, "distance", || widen_to_dense(points, triplets))?;
    let diagram = timed(phases, "reduce", || {
        rips_persistence(&distances, params).map_err(|error| error.to_string())
    })?;
    Ok((points, diagram, None))
}

fn distance_matrix(values: Parsed) -> Result<DistanceMatrix, String> {
    match values {
        Parsed::Points(points) => DistanceMatrix::from_points(&points),
        Parsed::Condensed(data) => DistanceMatrix::from_condensed(data),
        Parsed::Triplets(..) => unreachable!("triplets take their own arms"),
    }
    .map_err(|error| error.to_string())
}

fn reduce_dense_input(
    values: Parsed,
    engine: Engine,
    args: &Args,
    params: &RipsParams,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<(usize, Diagram, Option<usize>), String> {
    let distances = timed(phases, "distance", || distance_matrix(values))?;
    if distances.len() < 2 {
        return Err(format!(
            "{}: need at least two points",
            input::file_stem(&args.input)
        ));
    }
    let points = distances.len();
    match engine {
        Engine::Dense | Engine::Auto => {
            let diagram = timed(phases, "reduce", || {
                rips_persistence(&distances, params).map_err(|error| error.to_string())
            })?;
            Ok((points, diagram, None))
        }
        Engine::Sparse => reduce_thresholded(points, &distances, args, params, phases),
    }
}

fn reduce_thresholded(
    points: usize,
    distances: &DistanceMatrix,
    args: &Args,
    params: &RipsParams,
    phases: &mut Vec<(&'static str, f64)>,
) -> Result<(usize, Diagram, Option<usize>), String> {
    let sparse = timed(phases, "graph", || {
        input::threshold_to_sparse(distances, args.threshold)
    })?;
    let edges = sparse.num_edges();
    let diagram = timed(phases, "reduce", || {
        rips_persistence_sparse(&sparse, params).map_err(|error| error.to_string())
    })?;
    Ok((points, diagram, Some(edges)))
}

/// The full matrix of a sparse graph: every absent pair is +inf, which the
/// engine reads as an edge that never enters the filtration. The time and
/// memory of this call are the memory-stratum measurement.
fn widen_to_dense(n: usize, triplets: &[(usize, usize, f64)]) -> Result<DistanceMatrix, String> {
    SparseDistanceMatrix::from_triplets(n, triplets).map_err(|error| error.to_string())?;
    let mut data = vec![f64::INFINITY; n * n.saturating_sub(1) / 2];
    for &(i, j, d) in triplets {
        let (hi, lo) = if i > j { (i, j) } else { (j, i) };
        data[hi * (hi - 1) / 2 + lo] = d;
    }
    DistanceMatrix::from_condensed(data).map_err(|e| e.to_string())
}

fn configurations(args: &Args) -> Vec<Config> {
    args.mode
        .iter()
        .map(|&engine| Config {
            name: reporting::engine_name(engine),
            engine,
        })
        .collect()
}

fn diagrams_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(x, y)| {
            x.dim == y.dim
                && x.birth.to_bits() == y.birth.to_bits()
                && x.death.to_bits() == y.death.to_bits()
        })
}

#[cfg(test)]
mod tests {
    use super::widen_to_dense;

    #[test]
    fn dense_widening_rejects_conflicting_triplets() {
        let triplets = [(0, 1, 1.0), (1, 0, 2.0)];
        assert!(matches!(
            widen_to_dense(2, &triplets),
            Err(error) if error.contains("conflicting distances")
        ));
    }
}
