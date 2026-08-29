use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::relative::{decode_verified, verify_relative_composition};
use super::{ProofError, ProofLimits, Reader};

const MAGIC: &[u8; 8] = b"HOLOSDM\0";
const VERSION: u16 = 1;

/// Return true when bytes start with the distributed-interface manifest magic.
pub fn is_distributed_interface(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Counts from one checked distributed interface manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedDistributedInterface {
    /// Content-bound job identifier.
    pub job: [u8; 32],
    /// Final relative-interface artifact identifier.
    pub result: [u8; 32],
    /// Shard artifacts checked.
    pub shards: usize,
    /// Composition folds checked.
    pub folds: usize,
    /// Unique content-addressed objects checked.
    pub objects: usize,
    /// Highest checked homology dimension.
    pub max_dim: usize,
}

struct Manifest {
    job: [u8; 32],
    max_dim: usize,
    modulus: u32,
    separator: Vec<usize>,
    output_protected: Vec<usize>,
    shards: Vec<[u8; 32]>,
    folds: Vec<[u8; 32]>,
    result: [u8; 32],
}

/// Verify a `HOLOSDM` manifest and every referenced proof-exchange object.
///
/// `objects` can arrive in any order. Each object is identified by SHA-256.
/// The checker replays each keyed child-core union and the final protection
/// change.
pub fn verify_distributed_interface(
    manifest: &[u8],
    objects: &[Vec<u8>],
    limits: ProofLimits,
) -> Result<VerifiedDistributedInterface, ProofError> {
    let mut table = BTreeMap::new();
    for bytes in objects {
        let id: [u8; 32] = Sha256::digest(bytes).into();
        if table.insert(id, bytes.as_slice()).is_some() {
            return Err(ProofError::new(
                "distributed object list repeats an artifact",
            ));
        }
    }
    let checked = verify_distributed_interface_with(
        manifest,
        |id| {
            table
                .get(id)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| ProofError::new("distributed object is absent"))
        },
        limits,
    )?;
    if checked.objects != table.len() {
        return Err(ProofError::new(
            "distributed object set differs from the manifest references",
        ));
    }
    Ok(checked)
}

/// Verify a manifest while loading referenced objects by content identifier.
///
/// The checker retains at most three artifacts while it replays one fold.
/// It reads every distinct object once for its identifier and size, then reads
/// the objects needed by each composition step. The loader is an untrusted
/// byte source keyed by content identifier.
pub fn verify_distributed_interface_with<F>(
    manifest: &[u8],
    mut load: F,
    limits: ProofLimits,
) -> Result<VerifiedDistributedInterface, ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    if manifest.len() > limits.max_bytes {
        return Err(ProofError::new(
            "distributed manifest exceeds the byte limit",
        ));
    }
    let manifest = decode_manifest(manifest, limits)?;
    let referenced = referenced_objects(&manifest);
    verify_object_budget(&referenced, &mut load, limits)?;
    verify_shards(&manifest, &mut load, limits)?;
    verify_folds(&manifest, &mut load, limits)?;
    let last = *manifest.folds.last().expect("a checked manifest has folds");
    verify_result(&manifest, last, &mut load, limits)?;
    Ok(VerifiedDistributedInterface {
        job: manifest.job,
        result: manifest.result,
        shards: manifest.shards.len(),
        folds: manifest.folds.len().saturating_sub(1) + usize::from(manifest.result != last),
        objects: referenced.len(),
        max_dim: manifest.max_dim,
    })
}

fn referenced_objects(manifest: &Manifest) -> BTreeSet<[u8; 32]> {
    let mut referenced = manifest.shards.iter().copied().collect::<BTreeSet<_>>();
    referenced.extend(manifest.folds.iter().copied());
    referenced.insert(manifest.result);
    referenced
}

fn verify_object_budget<F>(
    referenced: &BTreeSet<[u8; 32]>,
    load: &mut F,
    limits: ProofLimits,
) -> Result<(), ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    let mut object_bytes = 0usize;
    for id in referenced {
        let bytes = load_checked(load, id, limits)?;
        object_bytes = object_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| ProofError::new("distributed object byte count overflows"))?;
        if object_bytes > limits.max_bytes {
            return Err(ProofError::new("distributed objects exceed the byte limit"));
        }
    }
    Ok(())
}

fn verify_shards<F>(
    manifest: &Manifest,
    load: &mut F,
    limits: ProofLimits,
) -> Result<(), ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    for shard in &manifest.shards {
        let bytes = load_checked(load, shard, limits)?;
        let certificate = decode_verified(&bytes, limits)?;
        if certificate.max_dim != manifest.max_dim
            || certificate.modulus != manifest.modulus
            || manifest.separator.iter().any(|vertex| {
                certificate
                    .protected_vertices
                    .binary_search(vertex)
                    .is_err()
            })
        {
            return Err(ProofError::new(
                "distributed shard has an incompatible algebra or separator",
            ));
        }
    }
    Ok(())
}

fn verify_folds<F>(manifest: &Manifest, load: &mut F, limits: ProofLimits) -> Result<(), ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    if manifest.folds[0] != manifest.shards[0] {
        return Err(ProofError::new(
            "first distributed fold differs from the first shard",
        ));
    }
    let mut intermediate = manifest.separator.clone();
    intermediate.extend_from_slice(&manifest.output_protected);
    intermediate.sort_unstable();
    intermediate.dedup();
    for position in 1..manifest.shards.len() {
        let parent = load_checked(load, &manifest.folds[position], limits)?;
        let left = load_checked(load, &manifest.folds[position - 1], limits)?;
        let right = load_checked(load, &manifest.shards[position], limits)?;
        verify_relative_composition(&parent, &[&left, &right], &intermediate, limits)?;
    }
    Ok(())
}

fn verify_result<F>(
    manifest: &Manifest,
    last: [u8; 32],
    load: &mut F,
    limits: ProofLimits,
) -> Result<(), ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    if manifest.result == last {
        let bytes = load_checked(load, &last, limits)?;
        let certificate = decode_verified(&bytes, limits)?;
        if certificate.protected_vertices != manifest.output_protected {
            return Err(ProofError::new(
                "distributed result has the wrong protected vertex set",
            ));
        }
    } else {
        let result = load_checked(load, &manifest.result, limits)?;
        let child = load_checked(load, &last, limits)?;
        verify_relative_composition(&result, &[&child], &manifest.output_protected, limits)?;
    }
    Ok(())
}

fn load_checked<F>(load: &mut F, id: &[u8; 32], limits: ProofLimits) -> Result<Vec<u8>, ProofError>
where
    F: FnMut(&[u8; 32]) -> Result<Vec<u8>, ProofError>,
{
    let bytes = load(id)?;
    if bytes.len() > limits.max_bytes {
        return Err(ProofError::new("distributed object exceeds the byte limit"));
    }
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != *id {
        return Err(ProofError::new(
            "distributed object fails its content identifier",
        ));
    }
    Ok(bytes)
}

fn decode_manifest(bytes: &[u8], limits: ProofLimits) -> Result<Manifest, ProofError> {
    let mut reader = Reader::new(bytes);
    decode_manifest_prefix(&mut reader)?;
    let manifest = decode_manifest_body(&mut reader, limits)?;
    validate_manifest(&manifest, reader.remaining())?;
    Ok(manifest)
}

fn decode_manifest_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION {
        return Err(ProofError::new(
            "unsupported distributed manifest magic or version",
        ));
    }
    Ok(())
}

fn decode_manifest_body(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Manifest, ProofError> {
    Ok(Manifest {
        job: reader.array32()?,
        max_dim: reader.bounded_usize("distributed dimension", limits.max_dimension)?,
        modulus: reader.u32()?,
        separator: decode_usizes(reader, limits.max_vertices)?,
        output_protected: decode_usizes(reader, limits.max_vertices)?,
        shards: decode_ids(reader, limits.max_references)?,
        folds: decode_ids(reader, limits.max_references)?,
        result: reader.array32()?,
    })
}

fn validate_manifest(manifest: &Manifest, remaining: usize) -> Result<(), ProofError> {
    if remaining != 0 || manifest.shards.is_empty() || manifest.folds.len() != manifest.shards.len()
    {
        return Err(ProofError::new("distributed manifest shape is invalid"));
    }
    if manifest.separator.windows(2).any(|pair| pair[0] >= pair[1])
        || manifest
            .output_protected
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "distributed manifest vertex lists are not canonical",
        ));
    }
    let computed = job_id(
        manifest.max_dim,
        manifest.modulus,
        &manifest.separator,
        &manifest.output_protected,
        &manifest.shards,
    );
    if computed != manifest.job {
        return Err(ProofError::new(
            "distributed manifest job binding is invalid",
        ));
    }
    Ok(())
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("distributed vertex count", maximum)?;
    (0..count)
        .map(|_| reader.bounded_usize("distributed vertex", maximum))
        .collect()
}

fn decode_ids(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<[u8; 32]>, ProofError> {
    let count = reader.bounded_usize("distributed identifier count", maximum)?;
    (0..count).map(|_| reader.array32()).collect()
}

fn job_id(
    max_dim: usize,
    modulus: u32,
    separator: &[usize],
    output_protected: &[usize],
    shards: &[[u8; 32]],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-distributed-interface-job-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, separator);
    digest_usizes(&mut hash, output_protected);
    hash.update((shards.len() as u64).to_be_bytes());
    for shard in shards {
        hash.update(shard);
    }
    hash.finalize().into()
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
    }
}
