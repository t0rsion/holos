use std::collections::BTreeSet;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use holos_tda::{
    ArtifactId, CertificateLimits, DurableInterfaceStore, GradedReductionCertificate,
    RelativeInterfaceCertificate, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};
use holos_tda_check::{ProofError, ProofLimits, verify_distributed_interface_with};

struct Args {
    shards: usize,
    gadgets: usize,
    reps: usize,
    modulus: u32,
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
        shards: parse("--shards", "8")?,
        gadgets: parse("--gadgets", "4")?,
        reps: parse("--reps", "5")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
    })
}

fn shard(
    shard: usize,
    shards: usize,
    gadgets: usize,
) -> Result<(SparseDistanceMatrix, Vec<usize>), String> {
    let first = 4 + shard * gadgets;
    let labels: Vec<_> = (0..4).chain(first..first + gadgets).collect();
    let mut edges = separator_edges();
    for local in 0..gadgets {
        let offset = shard * gadgets + local;
        let vertex = 4 + local;
        let side = offset % 4;
        let value = 2.0 + offset as f64 / (1000 * shards * gadgets) as f64;
        edges.push((side, vertex, value));
        edges.push(((side + 1) % 4, vertex, value));
    }
    SparseDistanceMatrix::from_triplets(4 + gadgets, &edges)
        .map(|graph| (graph, labels))
        .map_err(|error| error.to_string())
}

fn complete(shards: usize, gadgets: usize) -> Result<SparseDistanceMatrix, String> {
    let mut edges = separator_edges();
    for offset in 0..shards * gadgets {
        let vertex = 4 + offset;
        let side = offset % 4;
        let value = 2.0 + offset as f64 / (1000 * shards * gadgets) as f64;
        edges.push((side, vertex, value));
        edges.push(((side + 1) % 4, vertex, value));
    }
    SparseDistanceMatrix::from_triplets(4 + shards * gadgets, &edges)
        .map_err(|error| error.to_string())
}

fn separator_edges() -> Vec<(usize, usize, f64)> {
    vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn reset_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(path).map_err(|error| error.to_string())
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if args.reps < 5 || args.shards < 2 || args.gadgets == 0 {
        return Err("reps must be at least 5, shards at least 2, and gadgets positive".into());
    }
    let params = RipsParams::new(2).with_modulus(args.modulus);
    let limits = CertificateLimits::default();
    let separator = [0, 1, 2, 3];
    let mut certificates = Vec::with_capacity(args.shards);
    for position in 0..args.shards {
        let (graph, labels) = shard(position, args.shards, args.gadgets)?;
        certificates.push(
            RelativeInterfaceCertificate::build_labeled(
                &graph, &labels, &params, &separator, limits,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let artifacts: Vec<_> = certificates
        .iter()
        .map(|certificate| {
            certificate
                .encode(limits)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    let full = complete(args.shards, args.gadgets)?;
    let expected = rips_persistence_sparse(&full, &params).map_err(|error| error.to_string())?;
    let root = std::env::temp_dir().join(format!("holos_distributed_bench_{}", std::process::id()));
    reset_directory(&root)?;

    let mut clean_times = Vec::with_capacity(args.reps);
    let mut recovery_times = Vec::with_capacity(args.reps);
    let mut reference = None;
    for repetition in 0..args.reps {
        let path = root.join(format!("repetition-{repetition}"));
        let store = DurableInterfaceStore::open(&path).map_err(|error| error.to_string())?;
        let ids: Vec<_> = artifacts
            .iter()
            .map(|bytes| store.put(bytes).map(|value| value.0))
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let start = Instant::now();
        let commit = store
            .commit_stored(&ids, &separator, &[], limits)
            .map_err(|error| error.to_string())?;
        clean_times.push(start.elapsed().as_nanos());
        if commit.certificate().diagram().bars != expected.bars {
            return Err("distributed result differs from complete persistence".into());
        }
        let manifest_path = path
            .join("manifests")
            .join(format!("{}.hdm", commit.manifest().job()));
        std::fs::remove_file(manifest_path).map_err(|error| error.to_string())?;
        let start = Instant::now();
        let recovered = store
            .commit_stored(&ids, &separator, &[], limits)
            .map_err(|error| error.to_string())?;
        recovery_times.push(start.elapsed().as_nanos());
        if recovered.work().folds_reused != args.shards
            || recovered.work().folds_computed != 0
            || recovered.manifest() != commit.manifest()
        {
            return Err("durable recovery did not reuse the complete fold prefix".into());
        }
        if repetition == 0 {
            reference = Some((store, recovered, commit.work()));
        }
    }
    let (store, commit, clean_work) = reference.expect("at least five repetitions ran");
    let manifest = commit
        .manifest()
        .encode()
        .map_err(|error| error.to_string())?;
    let mut check_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        let checked = verify_distributed_interface_with(
            &manifest,
            |id| {
                store
                    .get(ArtifactId::from_bytes(*id), limits.max_bytes)
                    .map_err(|error| ProofError::new(error.to_string()))
            },
            ProofLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        black_box(checked);
        check_times.push(start.elapsed().as_nanos());
    }
    let mut materialized_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(
            GradedReductionCertificate::build(&full, &params, limits)
                .map_err(|error| error.to_string())?,
        );
        materialized_times.push(start.elapsed().as_nanos());
    }
    let mut objects: BTreeSet<_> = commit.manifest().shards().iter().copied().collect();
    objects.extend(commit.manifest().folds().iter().copied());
    objects.insert(commit.manifest().result());
    let object_bytes = objects
        .iter()
        .map(|id| store.get(*id, limits.max_bytes).map(|bytes| bytes.len()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .sum::<usize>();
    let input_cells = certificates
        .iter()
        .map(|item| item.work().input_cells)
        .sum::<usize>();
    let core_cells = certificates
        .iter()
        .map(|item| item.work().core_cells)
        .sum::<usize>();
    let cancellations = certificates
        .iter()
        .map(|item| item.work().cancellations)
        .sum::<usize>();
    let total_shard_bytes = artifacts.iter().map(Vec::len).sum::<usize>();
    println!(
        "format=holos-distributed-bench-v1 shards={} gadgets={} vertices={} modulus={} reps={} shard_input_cells={} shard_core_cells={} shard_cancellations={} total_shard_bytes={} peak_artifact_bytes={} object_bytes={} manifest_bytes={} objects={} clean_ns={} recovery_ns={} check_ns={} materialized_ns={} folds_reused={} h1_bars={}",
        args.shards,
        args.gadgets,
        full.len(),
        args.modulus,
        args.reps,
        input_cells,
        core_cells,
        cancellations,
        total_shard_bytes,
        clean_work.peak_artifact_bytes,
        object_bytes,
        manifest.len(),
        objects.len(),
        median(clean_times),
        median(recovery_times),
        median(check_times),
        median(materialized_times),
        commit.work().folds_reused,
        commit.certificate().diagram().in_dim(1).count(),
    );
    std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(())
}
