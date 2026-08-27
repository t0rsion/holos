use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CertificateLimits, GradedReductionCertificate, RelativeInterfaceCertificate, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse,
};
use holos_tda_check::{ProofLimits, verify_relative_interface};

struct Args {
    gadgets: usize,
    reps: usize,
    modulus: u32,
}

fn arguments() -> Result<Args, String> {
    let values: Vec<_> = std::env::args().skip(1).collect();
    let value = |name: &str, default: &str| -> Result<String, String> {
        Ok(values
            .windows(2)
            .find(|pair| pair[0] == name)
            .map_or(default, |pair| pair[1].as_str())
            .to_owned())
    };
    let parse = |name: &str, default: &str| -> Result<usize, String> {
        value(name, default)?
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    };
    Ok(Args {
        gadgets: parse("--gadgets", "8")?,
        reps: parse("--reps", "5")?,
        modulus: value("--modulus", "2")?
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
    })
}

fn child(gadgets: usize, first_label: usize) -> Result<(SparseDistanceMatrix, Vec<usize>), String> {
    let labels: Vec<_> = (0..4).chain(first_label..first_label + gadgets).collect();
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    for local in 4..4 + gadgets {
        let side = (local - 4) % 4;
        let value = 2.0 + (local - 4) as f64 / 1000.0;
        edges.push((side, local, value));
        edges.push(((side + 1) % 4, local, value));
    }
    SparseDistanceMatrix::from_triplets(4 + gadgets, &edges)
        .map(|graph| (graph, labels))
        .map_err(|error| error.to_string())
}

fn complete(gadgets: usize) -> Result<SparseDistanceMatrix, String> {
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    for offset in 0..2 * gadgets {
        let vertex = 4 + offset;
        let side = offset % 4;
        let value = 2.0 + (offset % gadgets) as f64 / 1000.0;
        edges.push((side, vertex, value));
        edges.push(((side + 1) % 4, vertex, value));
    }
    SparseDistanceMatrix::from_triplets(4 + 2 * gadgets, &edges).map_err(|error| error.to_string())
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if args.reps < 5 || args.gadgets == 0 {
        return Err("reps must be at least 5 and gadgets must be positive".into());
    }
    let params = RipsParams::new(2).with_modulus(args.modulus);
    let limits = CertificateLimits::default();
    let separator = [0, 1, 2, 3];
    let (left_graph, left_labels) = child(args.gadgets, 4)?;
    let (right_graph, right_labels) = child(args.gadgets, 4 + args.gadgets)?;
    let full = complete(args.gadgets)?;
    let left = RelativeInterfaceCertificate::build_labeled(
        &left_graph,
        &left_labels,
        &params,
        &separator,
        limits,
    )
    .map_err(|error| error.to_string())?;
    let right = RelativeInterfaceCertificate::build_labeled(
        &right_graph,
        &right_labels,
        &params,
        &separator,
        limits,
    )
    .map_err(|error| error.to_string())?;
    let expected = rips_persistence_sparse(&full, &params).map_err(|error| error.to_string())?;
    let composed = RelativeInterfaceCertificate::compose(&[&left, &right], &[], limits)
        .map_err(|error| error.to_string())?;
    if composed.diagram().bars != expected.bars || composed.diagram().in_dim(1).count() != 1 {
        return Err("relative composition differs from exact persistence".into());
    }
    let bytes = composed.encode(limits).map_err(|error| error.to_string())?;
    verify_relative_interface(&bytes, ProofLimits::default()).map_err(|error| error.to_string())?;

    let mut relative_times = Vec::with_capacity(args.reps);
    let mut materialized_times = Vec::with_capacity(args.reps);
    let mut check_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(
            RelativeInterfaceCertificate::compose(&[&left, &right], &[], limits)
                .map_err(|error| error.to_string())?,
        );
        relative_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(
            GradedReductionCertificate::build(&full, &params, limits)
                .map_err(|error| error.to_string())?,
        );
        materialized_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(
            verify_relative_interface(&bytes, ProofLimits::default())
                .map_err(|error| error.to_string())?,
        );
        check_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-interface-bench-v1 gadgets={} vertices={} modulus={} reps={} child_input_cells={} child_core_cells={} child_cancellations={} input_cells={} core_cells={} cancellations={} relative_ns={} materialized_ns={} proof_bytes={} check_ns={} h1_bars={}",
        args.gadgets,
        full.len(),
        args.modulus,
        args.reps,
        left.work().input_cells + right.work().input_cells,
        left.work().core_cells + right.work().core_cells,
        left.work().cancellations + right.work().cancellations,
        composed.work().input_cells,
        composed.work().core_cells,
        composed.work().cancellations,
        median(relative_times),
        median(materialized_times),
        bytes.len(),
        median(check_times),
        composed.diagram().in_dim(1).count(),
    );
    Ok(())
}
