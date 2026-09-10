use sha2::{Digest, Sha256};

use crate::{CertificateLimits, RelativeInterfaceCertificate};

use super::model::{
    ArtifactId, CommitPlan, DistributedInterfaceError, DistributedInterfaceWork, FoldState,
};
use super::wire::{PROGRESS_MAGIC, Reader, VERSION, digest_usizes};

pub(super) fn require_first_shard(
    shards: &[ArtifactId],
) -> Result<ArtifactId, DistributedInterfaceError> {
    shards
        .first()
        .copied()
        .ok_or_else(|| DistributedInterfaceError::new("distributed composition requires a shard"))
}

pub(super) fn check_progress_identity(
    reader: &mut Reader<'_>,
    job: ArtifactId,
) -> Result<(), DistributedInterfaceError> {
    let valid = reader.take(8)? == PROGRESS_MAGIC
        && reader.u16()? == VERSION
        && ArtifactId(reader.array32()?) == job;
    if !valid {
        return Err(DistributedInterfaceError::new(
            "durable fold progress has an invalid binding",
        ));
    }
    Ok(())
}

pub(super) fn check_progress_shape(
    remaining: usize,
    prefix: usize,
    accumulator: ArtifactId,
    folds: &[ArtifactId],
) -> Result<(), DistributedInterfaceError> {
    if remaining != 0 || folds.len() != prefix || folds.last().copied() != Some(accumulator) {
        return Err(DistributedInterfaceError::new(
            "durable fold progress has an invalid shape",
        ));
    }
    Ok(())
}

pub(super) fn combined_vertices(left: &[usize], right: &[usize]) -> Vec<usize> {
    let mut combined = left.to_vec();
    combined.extend_from_slice(right);
    combined.sort_unstable();
    combined.dedup();
    combined
}

pub(super) fn check_progress_prefix(
    prefix: usize,
    shard_count: usize,
) -> Result<(), DistributedInterfaceError> {
    if prefix == 0 || prefix > shard_count {
        return Err(DistributedInterfaceError::new(
            "durable fold progress has an invalid prefix",
        ));
    }
    Ok(())
}

pub(super) fn decode_certificate(
    bytes: &[u8],
    limits: CertificateLimits,
) -> Result<RelativeInterfaceCertificate, DistributedInterfaceError> {
    RelativeInterfaceCertificate::decode(bytes, limits)
        .map_err(|error| DistributedInterfaceError::new(error.to_string()))
}

pub(super) fn encode_certificate(
    certificate: &RelativeInterfaceCertificate,
    limits: CertificateLimits,
) -> Result<Vec<u8>, DistributedInterfaceError> {
    certificate
        .encode(limits)
        .map_err(|error| DistributedInterfaceError::new(error.to_string()))
}

pub(super) fn compose_certificates(
    children: &[&RelativeInterfaceCertificate],
    protected_vertices: &[usize],
    limits: CertificateLimits,
) -> Result<RelativeInterfaceCertificate, DistributedInterfaceError> {
    RelativeInterfaceCertificate::compose(children, protected_vertices, limits)
        .map_err(|error| DistributedInterfaceError::new(error.to_string()))
}

pub(super) fn compose_accumulator(
    plan: &CommitPlan<'_>,
    state: &mut FoldState,
    child: RelativeInterfaceCertificate,
    child_bytes: usize,
    work: &mut DistributedInterfaceWork,
) -> Result<(), DistributedInterfaceError> {
    require_compatible(&state.accumulator, &child)?;
    work.peak_artifact_bytes = work
        .peak_artifact_bytes
        .max(state.accumulator_bytes.len().saturating_add(child_bytes));
    state.accumulator = compose_certificates(
        &[&state.accumulator, &child],
        &plan.intermediate_protected,
        plan.limits,
    )?;
    state.accumulator_bytes = encode_certificate(&state.accumulator, plan.limits)?;
    Ok(())
}

pub(super) fn check_fold_id(
    bytes: &[u8],
    expected: ArtifactId,
) -> Result<(), DistributedInterfaceError> {
    if ArtifactId::for_bytes(bytes) != expected {
        return Err(DistributedInterfaceError::new(
            "manifest fold chain differs from recomputation",
        ));
    }
    Ok(())
}

pub(super) fn require_separator(
    certificate: &RelativeInterfaceCertificate,
    separator: &[usize],
) -> Result<(), DistributedInterfaceError> {
    if separator.iter().any(|vertex| {
        certificate
            .protected_vertices()
            .binary_search(vertex)
            .is_err()
    }) {
        return Err(DistributedInterfaceError::new(
            "shard does not protect every common separator vertex",
        ));
    }
    Ok(())
}

pub(super) fn require_compatible(
    left: &RelativeInterfaceCertificate,
    right: &RelativeInterfaceCertificate,
) -> Result<(), DistributedInterfaceError> {
    if left.max_dim() != right.max_dim() || left.modulus() != right.modulus() {
        return Err(DistributedInterfaceError::new(
            "shards use different dimensions or coefficient fields",
        ));
    }
    Ok(())
}

pub(super) fn job_id(
    max_dim: usize,
    modulus: u32,
    separator: &[usize],
    output_protected: &[usize],
    shards: &[ArtifactId],
) -> ArtifactId {
    let mut hash = Sha256::new();
    hash.update(b"holos-distributed-interface-job-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, separator);
    digest_usizes(&mut hash, output_protected);
    hash.update((shards.len() as u64).to_be_bytes());
    for shard in shards {
        hash.update(shard.as_bytes());
    }
    ArtifactId(hash.finalize().into())
}

pub(super) fn canonical_vertices(
    vertices: &[usize],
) -> Result<Vec<usize>, DistributedInterfaceError> {
    let mut result = vertices.to_vec();
    result.sort_unstable();
    result.dedup();
    if result.len() != vertices.len() {
        return Err(DistributedInterfaceError::new(
            "vertex list contains a duplicate",
        ));
    }
    Ok(result)
}

pub(super) fn require_canonical_vertices(
    vertices: &[usize],
) -> Result<(), DistributedInterfaceError> {
    if vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DistributedInterfaceError::new(
            "manifest vertex list is not canonical",
        ));
    }
    Ok(())
}
