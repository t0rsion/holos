use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::relative::{decode_verified, verify_relative_composition};
use super::{ProofError, ProofLimits, Reader};

const MAGIC: &[u8; 8] = b"HOLOSDM\0";
const VERSION: u16 = 1;

/// Whether bytes start with the distributed-interface manifest magic.
pub fn is_distributed_interface(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Counts from one independently replayed distributed interface manifest.
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
/// `objects` can arrive in any order. Every object is bound by SHA-256. The
/// checker replays each keyed child-core union and the final protection
/// change without linking to the producer crate.
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
/// the objects needed by each composition step. The callback can use a local
/// content-addressed store or another untrusted byte source.
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
    let mut referenced: BTreeSet<_> = manifest.shards.iter().copied().collect();
    referenced.extend(manifest.folds.iter().copied());
    referenced.insert(manifest.result);
    let mut object_bytes = 0usize;
    for id in &referenced {
        let bytes = load_checked(&mut load, id, limits)?;
        object_bytes = object_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| ProofError::new("distributed object byte count overflows"))?;
        if object_bytes > limits.max_bytes {
            return Err(ProofError::new("distributed objects exceed the byte limit"));
        }
    }

    for shard in &manifest.shards {
        let bytes = load_checked(&mut load, shard, limits)?;
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
        let parent = load_checked(&mut load, &manifest.folds[position], limits)?;
        let left = load_checked(&mut load, &manifest.folds[position - 1], limits)?;
        let right = load_checked(&mut load, &manifest.shards[position], limits)?;
        verify_relative_composition(&parent, &[&left, &right], &intermediate, limits)?;
    }
    let last = *manifest.folds.last().unwrap();
    if manifest.result == last {
        let bytes = load_checked(&mut load, &last, limits)?;
        let certificate = decode_verified(&bytes, limits)?;
        if certificate.protected_vertices != manifest.output_protected {
            return Err(ProofError::new(
                "distributed result has the wrong protected vertex set",
            ));
        }
    } else {
        let result = load_checked(&mut load, &manifest.result, limits)?;
        let child = load_checked(&mut load, &last, limits)?;
        verify_relative_composition(&result, &[&child], &manifest.output_protected, limits)?;
    }
    Ok(VerifiedDistributedInterface {
        job: manifest.job,
        result: manifest.result,
        shards: manifest.shards.len(),
        folds: manifest.folds.len().saturating_sub(1) + usize::from(manifest.result != last),
        objects: referenced.len(),
        max_dim: manifest.max_dim,
    })
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
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION {
        return Err(ProofError::new(
            "unsupported distributed manifest magic or version",
        ));
    }
    let job = reader.array32()?;
    let max_dim = reader.bounded_usize("distributed dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    let separator = decode_usizes(&mut reader, limits.max_vertices)?;
    let output_protected = decode_usizes(&mut reader, limits.max_vertices)?;
    let shards = decode_ids(&mut reader, limits.max_references)?;
    let folds = decode_ids(&mut reader, limits.max_references)?;
    let result = reader.array32()?;
    if reader.remaining() != 0 || shards.is_empty() || folds.len() != shards.len() {
        return Err(ProofError::new("distributed manifest shape is invalid"));
    }
    if separator.windows(2).any(|pair| pair[0] >= pair[1])
        || output_protected.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "distributed manifest vertex lists are not canonical",
        ));
    }
    let computed = job_id(max_dim, modulus, &separator, &output_protected, &shards);
    if computed != job {
        return Err(ProofError::new(
            "distributed manifest job binding is invalid",
        ));
    }
    Ok(Manifest {
        job,
        max_dim,
        modulus,
        separator,
        output_protected,
        shards,
        folds,
        result,
    })
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
