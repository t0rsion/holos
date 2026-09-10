//! Proof, index, and relative-interface command workflows.

use crate::{
    CertificateLimits, CorrespondenceMode, DurableInterfaceStore, IndexParams, IndexStream,
    IndexStreamProof, InterfacePolicy, PersistenceIndex, ProofArtifact,
    RelativeInterfaceCertificate, RipsParams,
};

use super::args::{IndexCli, InterfaceCli, MergeInterfacesCli, ProveCli};
use super::input::{invalid_input, read_bounded_artifact, read_proof_input, write_via_temporary};
use super::verify::certificate_limits;

pub(super) fn run_prove(cli: ProveCli) -> crate::Result<()> {
    let threads = cli.threads.max(1);
    let initial = read_proof_input(&cli.input, cli.format, threads, cli.threshold)?;
    let updates = cli
        .updates
        .iter()
        .map(|path| read_proof_input(path, cli.format, threads, cli.threshold))
        .collect::<crate::Result<Vec<_>>>()?;
    let params = RipsParams {
        max_dim: 1,
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads,
        ..RipsParams::default()
    };
    let proof = ProofArtifact::build(&initial, &updates, &params, CertificateLimits::default())
        .map_err(invalid_input)?;
    let summary = proof.summary();
    let bytes = proof.encode().map_err(invalid_input)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "wrote {} snapshots through {} unique reduction nodes; {} references reused",
        summary.snapshots, summary.unique_nodes, summary.reused_references
    );
    Ok(())
}

pub(super) fn run_index(cli: IndexCli) -> crate::Result<()> {
    validate_index_paths(&cli)?;
    let threads = cli.threads.max(1);
    let initial = read_proof_input(&cli.input, cli.format, threads, cli.threshold)?;
    let params = RipsParams {
        max_dim: cli.dim,
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads,
        ..RipsParams::default()
    };
    let index_params = IndexParams {
        max_separator_width: cli.separator_width,
        separator_search_limit: cli.separator_search_limit,
        leaf_vertices: cli.leaf_vertices,
        interface_policy: if cli.materialize_interfaces {
            InterfacePolicy::Materialize
        } else {
            InterfacePolicy::Relative
        },
    };
    let limits = CertificateLimits::default();
    let index = PersistenceIndex::compile(&initial, &params, index_params, limits)?;
    let mut stream = IndexStream::new(index);
    let snapshot = stream.checkpoint().map_err(invalid_input)?;
    let encoded = snapshot.encode().map_err(invalid_input)?;
    write_via_temporary(&cli.snapshot, &encoded)?;
    let snapshot_summary = snapshot.summary();
    let work = write_index_updates(&cli, threads, &mut stream)?;
    println!(
        "wrote one initial checkpoint through dimension {} with {} interfaces and {} records with {} changed interfaces, {} edge changes, and {} envelope checkpoints",
        cli.dim,
        snapshot_summary.nodes,
        cli.record.len(),
        work.changed_nodes,
        work.edge_changes,
        work.cold_records
    );
    Ok(())
}

pub(super) fn validate_index_paths(cli: &IndexCli) -> crate::Result<()> {
    if cli.update.len() != cli.record.len() {
        return Err(crate::Error::InvalidInput(format!(
            "--update occurs {} times but --record occurs {} times",
            cli.update.len(),
            cli.record.len()
        )));
    }
    Ok(())
}

#[derive(Default)]
struct IndexCliWork {
    changed_nodes: usize,
    edge_changes: usize,
    cold_records: usize,
}

fn write_index_updates(
    cli: &IndexCli,
    threads: usize,
    stream: &mut IndexStream,
) -> crate::Result<IndexCliWork> {
    let mut work = IndexCliWork::default();
    for (input, output) in cli.update.iter().zip(&cli.record) {
        let graph = read_proof_input(input, cli.format, threads, cli.threshold)?;
        let step = stream.apply_graph(&graph, CorrespondenceMode::Omit)?;
        let summary = match &step.proof {
            IndexStreamProof::Delta(proof) => proof.summary(),
            IndexStreamProof::Snapshot(proof) => {
                work.cold_records += 1;
                proof.summary()
            }
        };
        let bytes = step.proof.encode().map_err(invalid_input)?;
        write_via_temporary(output, &bytes)?;
        work.changed_nodes += summary.nodes;
        work.edge_changes += summary.edge_changes;
    }
    Ok(work)
}

pub(super) fn run_interface(cli: InterfaceCli) -> crate::Result<()> {
    let input = read_proof_input(&cli.input, cli.format, cli.threads, cli.threshold)?;
    let mut params = RipsParams::new(cli.dim).with_modulus(cli.modulus);
    params.threshold = cli.threshold;
    let certificate = RelativeInterfaceCertificate::build(
        &input,
        &params,
        &cli.protected,
        CertificateLimits::default(),
    )
    .map_err(invalid_input)?;
    let bytes = certificate
        .encode(CertificateLimits::default())
        .map_err(invalid_input)?;
    write_via_temporary(&cli.output, &bytes)?;
    let work = certificate.work();
    println!(
        "wrote relative interface through dimension {} with {} input cells, {} cancellations, {} retained cells, and {} bytes",
        cli.dim,
        work.input_cells,
        work.cancellations,
        work.core_cells,
        bytes.len(),
    );
    Ok(())
}

pub(super) fn run_merge_interfaces(cli: MergeInterfacesCli) -> crate::Result<()> {
    let store = DurableInterfaceStore::open(&cli.store).map_err(invalid_input)?;
    let mut shard_ids = Vec::with_capacity(cli.shards.len());
    for path in &cli.shards {
        let bytes = read_bounded_artifact(path, cli.max_artifact_bytes, "interface shard")?;
        let (id, _) = store.put(&bytes).map_err(invalid_input)?;
        shard_ids.push(id);
    }
    let limits = certificate_limits(cli.max_artifact_bytes);
    let commit = store
        .commit_stored(&shard_ids, &cli.separator, &cli.protected, limits)
        .map_err(invalid_input)?;
    let manifest = commit.manifest().encode().map_err(invalid_input)?;
    let result = commit.certificate().encode(limits).map_err(invalid_input)?;
    write_via_temporary(&cli.manifest, &manifest)?;
    write_via_temporary(&cli.result, &result)?;
    let work = commit.work();
    println!(
        "committed distributed interface {} with {} shards, {} reused folds, {} computed folds, and {} result bytes",
        commit.manifest().job(),
        work.shards,
        work.folds_reused,
        work.folds_computed,
        result.len(),
    );
    Ok(())
}
