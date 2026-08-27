//! Durable content-addressed execution for relative interfaces.
//!
//! A store writes verified shard artifacts under their SHA-256 identifiers.
//! The coordinator folds one shard at a time while protecting the common
//! separator. Each fold is durable before the next one starts. An atomic
//! manifest publishes the final result.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{CertificateLimits, RelativeInterfaceCertificate};

const MANIFEST_MAGIC: &[u8; 8] = b"HOLOSDM\0";
const PROGRESS_MAGIC: &[u8; 8] = b"HOLOSDW\0";
const VERSION: u16 = 1;

/// Failure during durable interface execution or recovery.
#[derive(Debug)]
pub struct DistributedInterfaceError {
    message: String,
}

impl DistributedInterfaceError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the failed store or algebra rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DistributedInterfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "distributed interface: {}", self.message)
    }
}

impl std::error::Error for DistributedInterfaceError {}

/// SHA-256 identifier of one stored artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactId([u8; 32]);

impl ArtifactId {
    /// Compute the identifier of exact artifact bytes.
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Construct an identifier from its 32-byte representation.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hexadecimal identifier.
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
        }
        output
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Exact I/O and fold work for one distributed composition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistributedInterfaceWork {
    /// Shard artifacts in the job.
    pub shards: usize,
    /// Shard artifacts decoded during this invocation.
    pub shards_loaded: usize,
    /// Completed folds reused from durable progress.
    pub folds_reused: usize,
    /// New folds computed during this invocation.
    pub folds_computed: usize,
    /// Artifact bytes read from the store.
    pub bytes_read: usize,
    /// New artifact bytes written to the store.
    pub bytes_written: usize,
    /// Largest accumulator-plus-shard byte count during one fold.
    pub peak_artifact_bytes: usize,
}

/// Immutable commit record for one distributed interface result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributedInterfaceManifest {
    job: ArtifactId,
    max_dim: usize,
    modulus: u32,
    separator_vertices: Vec<usize>,
    output_protected_vertices: Vec<usize>,
    shards: Vec<ArtifactId>,
    folds: Vec<ArtifactId>,
    result: ArtifactId,
}

impl DistributedInterfaceManifest {
    /// Deterministic job identifier.
    pub fn job(&self) -> ArtifactId {
        self.job
    }

    /// Highest composed homology dimension.
    pub fn max_dim(&self) -> usize {
        self.max_dim
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Separator protected during every intermediate fold.
    pub fn separator_vertices(&self) -> &[usize] {
        &self.separator_vertices
    }

    /// Protected vertices retained in the final result.
    pub fn output_protected_vertices(&self) -> &[usize] {
        &self.output_protected_vertices
    }

    /// Ordered shard artifact identifiers.
    pub fn shards(&self) -> &[ArtifactId] {
        &self.shards
    }

    /// Accumulator identifier after each shard.
    pub fn folds(&self) -> &[ArtifactId] {
        &self.folds
    }

    /// Final relative-interface artifact identifier.
    pub fn result(&self) -> ArtifactId {
        self.result
    }

    /// Encode the canonical `HOLOSDM` version 1 manifest.
    pub fn encode(&self) -> Result<Vec<u8>, DistributedInterfaceError> {
        let mut output = Vec::new();
        output.extend_from_slice(MANIFEST_MAGIC);
        output.extend_from_slice(&VERSION.to_be_bytes());
        output.extend_from_slice(self.job.as_bytes());
        put_usize(&mut output, self.max_dim)?;
        output.extend_from_slice(&self.modulus.to_be_bytes());
        encode_usizes(&mut output, &self.separator_vertices)?;
        encode_usizes(&mut output, &self.output_protected_vertices)?;
        encode_ids(&mut output, &self.shards)?;
        encode_ids(&mut output, &self.folds)?;
        output.extend_from_slice(self.result.as_bytes());
        Ok(output)
    }

    /// Decode one canonical bounded `HOLOSDM` version 1 manifest.
    pub fn decode(bytes: &[u8], maximum_bytes: usize) -> Result<Self, DistributedInterfaceError> {
        if bytes.len() > maximum_bytes {
            return Err(DistributedInterfaceError::new(
                "manifest exceeds the byte limit",
            ));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MANIFEST_MAGIC || reader.u16()? != VERSION {
            return Err(DistributedInterfaceError::new(
                "unsupported distributed manifest",
            ));
        }
        let job = ArtifactId(reader.array32()?);
        let max_dim = reader.usize()?;
        let modulus = reader.u32()?;
        let separator_vertices = decode_usizes(&mut reader)?;
        let output_protected_vertices = decode_usizes(&mut reader)?;
        let shards = decode_ids(&mut reader)?;
        let folds = decode_ids(&mut reader)?;
        let result = ArtifactId(reader.array32()?);
        if reader.remaining() != 0 || shards.is_empty() || folds.len() != shards.len() {
            return Err(DistributedInterfaceError::new(
                "distributed manifest shape is invalid",
            ));
        }
        require_canonical_vertices(&separator_vertices)?;
        require_canonical_vertices(&output_protected_vertices)?;
        let expected_job = job_id(
            max_dim,
            modulus,
            &separator_vertices,
            &output_protected_vertices,
            &shards,
        );
        if expected_job != job {
            return Err(DistributedInterfaceError::new(
                "distributed manifest binding is invalid",
            ));
        }
        Ok(Self {
            job,
            max_dim,
            modulus,
            separator_vertices,
            output_protected_vertices,
            shards,
            folds,
            result,
        })
    }
}

/// Completed distributed composition and its durable manifest.
#[derive(Debug, Clone)]
pub struct DistributedInterfaceCommit {
    manifest: DistributedInterfaceManifest,
    certificate: RelativeInterfaceCertificate,
    work: DistributedInterfaceWork,
}

impl DistributedInterfaceCommit {
    /// Durable commit manifest.
    pub fn manifest(&self) -> &DistributedInterfaceManifest {
        &self.manifest
    }

    /// Composed relative-interface certificate.
    pub fn certificate(&self) -> &RelativeInterfaceCertificate {
        &self.certificate
    }

    /// I/O and fold work charged to this invocation.
    pub fn work(&self) -> DistributedInterfaceWork {
        self.work
    }
}

/// Filesystem-backed content-addressed store for interface artifacts.
#[derive(Debug, Clone)]
pub struct DurableInterfaceStore {
    root: PathBuf,
}

impl DurableInterfaceStore {
    /// Open or create a store rooted at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DistributedInterfaceError> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("objects"))
            .and_then(|_| fs::create_dir_all(root.join("jobs")))
            .and_then(|_| fs::create_dir_all(root.join("manifests")))
            .map_err(|error| DistributedInterfaceError::new(format!("create store: {error}")))?;
        Ok(Self { root })
    }

    /// Store exact bytes and return their content identifier.
    pub fn put(&self, bytes: &[u8]) -> Result<(ArtifactId, bool), DistributedInterfaceError> {
        let id = ArtifactId::for_bytes(bytes);
        let path = self.object_path(id);
        if path.is_file() {
            let existing = read_bounded(&path, bytes.len())?;
            if existing != bytes {
                return Err(DistributedInterfaceError::new(
                    "stored object differs under the same content identifier",
                ));
            }
            return Ok((id, false));
        }
        let parent = path.parent().expect("object path has a parent");
        fs::create_dir_all(parent).map_err(|error| {
            DistributedInterfaceError::new(format!("create object path: {error}"))
        })?;
        sync_parent(parent)?;
        sync_directory(parent)?;
        atomic_write(&path, bytes)?;
        Ok((id, true))
    }

    /// Read an object and check its identifier.
    pub fn get(
        &self,
        id: ArtifactId,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, DistributedInterfaceError> {
        let bytes = read_bounded(&self.object_path(id), maximum_bytes)?;
        if ArtifactId::for_bytes(&bytes) != id {
            return Err(DistributedInterfaceError::new(
                "stored object fails its content identifier",
            ));
        }
        Ok(bytes)
    }

    /// Whether an object path exists. This does not read or verify the bytes.
    pub fn contains(&self, id: ArtifactId) -> bool {
        self.object_path(id).is_file()
    }

    /// Compose shard artifacts and publish one atomic durable manifest.
    ///
    /// Shards are ordered. Every intermediate fold protects the union of
    /// `separator_vertices` and `output_protected_vertices`. A retry resumes
    /// from the last durable fold for the same content-bound job.
    pub fn commit(
        &self,
        shard_artifacts: &[Vec<u8>],
        separator_vertices: &[usize],
        output_protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<DistributedInterfaceCommit, DistributedInterfaceError> {
        let mut shard_ids = Vec::with_capacity(shard_artifacts.len());
        let mut work = DistributedInterfaceWork {
            shards: shard_artifacts.len(),
            ..DistributedInterfaceWork::default()
        };
        for bytes in shard_artifacts {
            let (id, written) = self.put(bytes)?;
            shard_ids.push(id);
            if written {
                work.bytes_written += bytes.len();
            }
        }
        self.commit_stored_inner(
            &shard_ids,
            separator_vertices,
            output_protected_vertices,
            limits,
            work,
        )
    }

    /// Compose shards already present in this store.
    ///
    /// The coordinator loads at most one shard and one accumulator artifact
    /// for each fold. The identifiers remain ordered and bind the job.
    pub fn commit_stored(
        &self,
        shard_ids: &[ArtifactId],
        separator_vertices: &[usize],
        output_protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<DistributedInterfaceCommit, DistributedInterfaceError> {
        self.commit_stored_inner(
            shard_ids,
            separator_vertices,
            output_protected_vertices,
            limits,
            DistributedInterfaceWork {
                shards: shard_ids.len(),
                ..DistributedInterfaceWork::default()
            },
        )
    }

    fn commit_stored_inner(
        &self,
        shard_ids: &[ArtifactId],
        separator_vertices: &[usize],
        output_protected_vertices: &[usize],
        limits: CertificateLimits,
        mut work: DistributedInterfaceWork,
    ) -> Result<DistributedInterfaceCommit, DistributedInterfaceError> {
        let Some(first_id) = shard_ids.first().copied() else {
            return Err(DistributedInterfaceError::new(
                "distributed composition requires a shard",
            ));
        };
        let separator_vertices = canonical_vertices(separator_vertices)?;
        let output_protected_vertices = canonical_vertices(output_protected_vertices)?;
        let first_bytes = self.get(first_id, limits.max_bytes)?;
        work.bytes_read += first_bytes.len();
        work.peak_artifact_bytes = first_bytes.len();
        let first = RelativeInterfaceCertificate::decode(&first_bytes, limits)
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        work.shards_loaded += 1;
        require_separator(&first, &separator_vertices)?;
        let job = job_id(
            first.max_dim(),
            first.modulus(),
            &separator_vertices,
            &output_protected_vertices,
            shard_ids,
        );
        if let Some(manifest) = self.read_manifest(job, limits.max_bytes)? {
            return self.load_commit(manifest, work, limits);
        }

        let progress = self.read_progress(job, limits.max_bytes)?;
        let (mut accumulator, mut folds, start) = if let Some(progress) = progress {
            if progress.prefix == 0 || progress.prefix > shard_ids.len() {
                return Err(DistributedInterfaceError::new(
                    "durable fold progress has an invalid prefix",
                ));
            }
            let bytes = self.get(progress.accumulator, limits.max_bytes)?;
            work.bytes_read += bytes.len();
            let accumulator = RelativeInterfaceCertificate::decode(&bytes, limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            work.folds_reused = progress.prefix;
            (accumulator, progress.folds, progress.prefix)
        } else {
            let folds = vec![first_id];
            self.write_progress(
                job,
                &Progress {
                    prefix: 1,
                    accumulator: first_id,
                    folds: folds.clone(),
                },
            )?;
            (first, folds, 1)
        };

        let mut intermediate_protected = separator_vertices.clone();
        intermediate_protected.extend(output_protected_vertices.iter().copied());
        intermediate_protected.sort_unstable();
        intermediate_protected.dedup();
        let mut accumulator_bytes = accumulator
            .encode(limits)
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        for (position, shard_id) in shard_ids.iter().enumerate().skip(start) {
            let bytes = self.get(*shard_id, limits.max_bytes)?;
            work.bytes_read += bytes.len();
            let child = RelativeInterfaceCertificate::decode(&bytes, limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            work.shards_loaded += 1;
            require_compatible(&accumulator, &child)?;
            require_separator(&child, &separator_vertices)?;
            work.peak_artifact_bytes = work
                .peak_artifact_bytes
                .max(accumulator_bytes.len().saturating_add(bytes.len()));
            accumulator = RelativeInterfaceCertificate::compose(
                &[&accumulator, &child],
                &intermediate_protected,
                limits,
            )
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            accumulator_bytes = accumulator
                .encode(limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            let (fold, written) = self.put(&accumulator_bytes)?;
            if written {
                work.bytes_written += accumulator_bytes.len();
            }
            folds.push(fold);
            work.folds_computed += 1;
            self.write_progress(
                job,
                &Progress {
                    prefix: position + 1,
                    accumulator: fold,
                    folds: folds.clone(),
                },
            )?;
        }
        if accumulator.protected_vertices() != output_protected_vertices {
            accumulator = RelativeInterfaceCertificate::compose(
                &[&accumulator],
                &output_protected_vertices,
                limits,
            )
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            accumulator_bytes = accumulator
                .encode(limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            let (_, written) = self.put(&accumulator_bytes)?;
            if written {
                work.bytes_written += accumulator_bytes.len();
            }
        }
        let result = ArtifactId::for_bytes(&accumulator_bytes);
        let manifest = DistributedInterfaceManifest {
            job,
            max_dim: accumulator.max_dim(),
            modulus: accumulator.modulus(),
            separator_vertices,
            output_protected_vertices,
            shards: shard_ids.to_vec(),
            folds,
            result,
        };
        let manifest_bytes = manifest.encode()?;
        atomic_write(&self.manifest_path(job), &manifest_bytes)?;
        Ok(DistributedInterfaceCommit {
            manifest,
            certificate: accumulator,
            work,
        })
    }

    /// Recompute every fold and verify a committed manifest from stored shards.
    ///
    /// This path ignores durable progress. It checks that every recorded fold
    /// and the final result follow from the ordered shard artifacts.
    pub fn verify_manifest(
        &self,
        manifest: &DistributedInterfaceManifest,
        limits: CertificateLimits,
    ) -> Result<RelativeInterfaceCertificate, DistributedInterfaceError> {
        let first_bytes = self.get(manifest.shards[0], limits.max_bytes)?;
        let mut accumulator = RelativeInterfaceCertificate::decode(&first_bytes, limits)
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        require_separator(&accumulator, &manifest.separator_vertices)?;
        if ArtifactId::for_bytes(&first_bytes) != manifest.folds[0] {
            return Err(DistributedInterfaceError::new(
                "manifest fold chain differs from recomputation",
            ));
        }
        let mut intermediate = manifest.separator_vertices.clone();
        intermediate.extend_from_slice(&manifest.output_protected_vertices);
        intermediate.sort_unstable();
        intermediate.dedup();
        for (position, shard) in manifest.shards.iter().enumerate().skip(1) {
            let bytes = self.get(*shard, limits.max_bytes)?;
            let child = RelativeInterfaceCertificate::decode(&bytes, limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            require_compatible(&accumulator, &child)?;
            require_separator(&child, &manifest.separator_vertices)?;
            accumulator = RelativeInterfaceCertificate::compose(
                &[&accumulator, &child],
                &intermediate,
                limits,
            )
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            let bytes = accumulator
                .encode(limits)
                .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
            if ArtifactId::for_bytes(&bytes) != manifest.folds[position] {
                return Err(DistributedInterfaceError::new(
                    "manifest fold chain differs from recomputation",
                ));
            }
        }
        if accumulator.protected_vertices() != manifest.output_protected_vertices {
            accumulator = RelativeInterfaceCertificate::compose(
                &[&accumulator],
                &manifest.output_protected_vertices,
                limits,
            )
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        }
        let encoded = accumulator
            .encode(limits)
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        if ArtifactId::for_bytes(&encoded) != manifest.result {
            return Err(DistributedInterfaceError::new(
                "manifest result differs from recomputation",
            ));
        }
        let stored = self.get(manifest.result, limits.max_bytes)?;
        if stored != encoded {
            return Err(DistributedInterfaceError::new(
                "stored result differs from recomputed result",
            ));
        }
        Ok(accumulator)
    }

    /// Read a committed manifest by job identifier.
    pub fn manifest(
        &self,
        job: ArtifactId,
        maximum_bytes: usize,
    ) -> Result<DistributedInterfaceManifest, DistributedInterfaceError> {
        self.read_manifest(job, maximum_bytes)?
            .ok_or_else(|| DistributedInterfaceError::new("distributed manifest is absent"))
    }

    fn load_commit(
        &self,
        manifest: DistributedInterfaceManifest,
        mut work: DistributedInterfaceWork,
        limits: CertificateLimits,
    ) -> Result<DistributedInterfaceCommit, DistributedInterfaceError> {
        let bytes = self.get(manifest.result, limits.max_bytes)?;
        work.bytes_read += bytes.len();
        work.folds_reused = manifest.folds.len();
        let certificate = RelativeInterfaceCertificate::decode(&bytes, limits)
            .map_err(|error| DistributedInterfaceError::new(error.to_string()))?;
        Ok(DistributedInterfaceCommit {
            manifest,
            certificate,
            work,
        })
    }

    fn object_path(&self, id: ArtifactId) -> PathBuf {
        let hex = id.to_hex();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    fn manifest_path(&self, id: ArtifactId) -> PathBuf {
        self.root.join("manifests").join(format!("{id}.hdm"))
    }

    fn progress_path(&self, id: ArtifactId) -> PathBuf {
        self.root.join("jobs").join(format!("{id}.work"))
    }

    fn read_manifest(
        &self,
        id: ArtifactId,
        maximum_bytes: usize,
    ) -> Result<Option<DistributedInterfaceManifest>, DistributedInterfaceError> {
        let path = self.manifest_path(id);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = read_bounded(&path, maximum_bytes)?;
        DistributedInterfaceManifest::decode(&bytes, maximum_bytes).map(Some)
    }

    fn write_progress(
        &self,
        job: ArtifactId,
        progress: &Progress,
    ) -> Result<(), DistributedInterfaceError> {
        let mut output = Vec::new();
        output.extend_from_slice(PROGRESS_MAGIC);
        output.extend_from_slice(&VERSION.to_be_bytes());
        output.extend_from_slice(job.as_bytes());
        put_usize(&mut output, progress.prefix)?;
        output.extend_from_slice(progress.accumulator.as_bytes());
        encode_ids(&mut output, &progress.folds)?;
        atomic_replace(&self.progress_path(job), &output)
    }

    fn read_progress(
        &self,
        job: ArtifactId,
        maximum_bytes: usize,
    ) -> Result<Option<Progress>, DistributedInterfaceError> {
        let path = self.progress_path(job);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = read_bounded(&path, maximum_bytes)?;
        let mut reader = Reader::new(&bytes);
        if reader.take(8)? != PROGRESS_MAGIC
            || reader.u16()? != VERSION
            || ArtifactId(reader.array32()?) != job
        {
            return Err(DistributedInterfaceError::new(
                "durable fold progress has an invalid binding",
            ));
        }
        let prefix = reader.usize()?;
        let accumulator = ArtifactId(reader.array32()?);
        let folds = decode_ids(&mut reader)?;
        if reader.remaining() != 0
            || folds.len() != prefix
            || folds.last().copied() != Some(accumulator)
        {
            return Err(DistributedInterfaceError::new(
                "durable fold progress has an invalid shape",
            ));
        }
        Ok(Some(Progress {
            prefix,
            accumulator,
            folds,
        }))
    }
}

struct Progress {
    prefix: usize,
    accumulator: ArtifactId,
    folds: Vec<ArtifactId>,
}

fn require_separator(
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

fn require_compatible(
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

fn job_id(
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

fn canonical_vertices(vertices: &[usize]) -> Result<Vec<usize>, DistributedInterfaceError> {
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

fn require_canonical_vertices(vertices: &[usize]) -> Result<(), DistributedInterfaceError> {
    if vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DistributedInterfaceError::new(
            "manifest vertex list is not canonical",
        ));
    }
    Ok(())
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, DistributedInterfaceError> {
    let file = File::open(path).map_err(|error| {
        DistributedInterfaceError::new(format!("read {}: {error}", path.display()))
    })?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| DistributedInterfaceError::new(format!("read object: {error}")))?;
    if bytes.len() > maximum {
        return Err(DistributedInterfaceError::new(
            "stored artifact exceeds the byte limit",
        ));
    }
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), DistributedInterfaceError> {
    for nonce in 0..100u32 {
        let temporary = temporary_path(path, nonce);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(DistributedInterfaceError::new(format!(
                    "create {}: {error}",
                    temporary.display()
                )));
            }
        };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| DistributedInterfaceError::new(format!("write object: {error}")))?;
        match fs::rename(&temporary, path) {
            Ok(()) => {
                sync_parent(path)?;
                return Ok(());
            }
            Err(error) if path.is_file() => {
                let _ = fs::remove_file(&temporary);
                let existing = read_bounded(path, bytes.len())?;
                if existing == bytes {
                    return Ok(());
                }
                return Err(DistributedInterfaceError::new(
                    "atomic destination contains different bytes",
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(DistributedInterfaceError::new(format!(
                    "publish {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Err(DistributedInterfaceError::new(
        "cannot reserve an atomic temporary path",
    ))
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), DistributedInterfaceError> {
    for nonce in 0..100u32 {
        let temporary = temporary_path(path, nonce);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(DistributedInterfaceError::new(format!(
                    "create progress: {error}"
                )));
            }
        };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| DistributedInterfaceError::new(format!("write progress: {error}")))?;
        fs::rename(&temporary, path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            DistributedInterfaceError::new(format!("publish progress: {error}"))
        })?;
        sync_parent(path)?;
        return Ok(());
    }
    Err(DistributedInterfaceError::new(
        "cannot reserve a progress temporary path",
    ))
}

fn temporary_path(path: &Path, nonce: u32) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{}.{nonce}.tmp", std::process::id()))
}

fn sync_parent(path: &Path) -> Result<(), DistributedInterfaceError> {
    let parent = path
        .parent()
        .ok_or_else(|| DistributedInterfaceError::new("durable path has no parent"))?;
    sync_directory(parent)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), DistributedInterfaceError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            DistributedInterfaceError::new(format!(
                "synchronize directory {}: {error}",
                path.display()
            ))
        })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), DistributedInterfaceError> {
    Ok(())
}

fn encode_ids(
    output: &mut Vec<u8>,
    values: &[ArtifactId],
) -> Result<(), DistributedInterfaceError> {
    put_usize(output, values.len())?;
    for value in values {
        output.extend_from_slice(value.as_bytes());
    }
    Ok(())
}

fn decode_ids(reader: &mut Reader<'_>) -> Result<Vec<ArtifactId>, DistributedInterfaceError> {
    let count = reader.usize()?;
    let maximum = reader.remaining() / 32;
    if count > maximum {
        return Err(DistributedInterfaceError::new(
            "manifest identifier count exceeds the remaining bytes",
        ));
    }
    (0..count)
        .map(|_| reader.array32().map(ArtifactId))
        .collect()
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), DistributedInterfaceError> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

fn decode_usizes(reader: &mut Reader<'_>) -> Result<Vec<usize>, DistributedInterfaceError> {
    let count = reader.usize()?;
    let maximum = reader.remaining() / 8;
    if count > maximum {
        return Err(DistributedInterfaceError::new(
            "manifest integer count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), DistributedInterfaceError> {
    let value = u64::try_from(value)
        .map_err(|_| DistributedInterfaceError::new("integer does not fit the wire format"))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], DistributedInterfaceError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| DistributedInterfaceError::new("manifest position overflows"))?;
        if end > self.bytes.len() {
            return Err(DistributedInterfaceError::new("manifest is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, DistributedInterfaceError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, DistributedInterfaceError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, DistributedInterfaceError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn usize(&mut self) -> Result<usize, DistributedInterfaceError> {
        usize::try_from(self.u64()?)
            .map_err(|_| DistributedInterfaceError::new("manifest integer does not fit usize"))
    }

    fn array32(&mut self) -> Result<[u8; 32], DistributedInterfaceError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_sparse};
    use holos_tda_check::{ProofLimits, verify_distributed_interface};
    use std::collections::BTreeSet;

    struct TestStore(PathBuf);

    impl TestStore {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "holos_distributed_{}_{}",
                std::process::id(),
                name
            ));
            if path.exists() {
                fs::remove_dir_all(&path).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn shard(labels: &[usize], side: usize, modulus: u32) -> RelativeInterfaceCertificate {
        let graph = SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (side, 4, 2.0),
                ((side + 1) % 4, 4, 2.0),
            ],
        )
        .unwrap();
        RelativeInterfaceCertificate::build_labeled(
            &graph,
            labels,
            &RipsParams::new(2).with_modulus(modulus),
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn commits_recovers_and_independently_replays_a_streaming_fold() {
        let temporary = TestStore::new("commit");
        let store = DurableInterfaceStore::open(&temporary.0).unwrap();
        let shards = [
            shard(&[0, 1, 2, 3, 4], 0, 5),
            shard(&[0, 1, 2, 3, 5], 1, 5),
            shard(&[0, 1, 2, 3, 6], 2, 5),
        ];
        let artifacts: Vec<_> = shards
            .iter()
            .map(|shard| shard.encode(CertificateLimits::default()).unwrap())
            .collect();
        let commit = store
            .commit(&artifacts, &[0, 1, 2, 3], &[], CertificateLimits::default())
            .unwrap();
        assert_eq!(commit.manifest().shards().len(), 3);
        assert_eq!(commit.manifest().folds().len(), 3);
        assert_eq!(commit.work().folds_computed, 2);
        let verified = store
            .verify_manifest(commit.manifest(), CertificateLimits::default())
            .unwrap();
        assert_eq!(verified.diagram().bars, commit.certificate().diagram().bars);
        let mut ids: BTreeSet<_> = commit.manifest().shards().iter().copied().collect();
        ids.extend(commit.manifest().folds().iter().copied());
        ids.insert(commit.manifest().result());
        let objects: Vec<_> = ids
            .into_iter()
            .map(|id| {
                store
                    .get(id, CertificateLimits::default().max_bytes)
                    .unwrap()
            })
            .collect();
        let independent = verify_distributed_interface(
            &commit.manifest().encode().unwrap(),
            &objects,
            ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(independent.result, *commit.manifest().result().as_bytes());
        assert_eq!(independent.shards, 3);

        fs::remove_file(store.manifest_path(commit.manifest().job())).unwrap();
        let recovered = store
            .commit(&artifacts, &[0, 1, 2, 3], &[], CertificateLimits::default())
            .unwrap();
        assert_eq!(recovered.manifest(), commit.manifest());
        assert_eq!(recovered.work().folds_reused, 3);
        assert_eq!(recovered.work().folds_computed, 0);

        let mut full_edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
        full_edges.extend([(0, 4, 2.0), (1, 4, 2.0)]);
        full_edges.extend([(1, 5, 2.0), (2, 5, 2.0)]);
        full_edges.extend([(2, 6, 2.0), (3, 6, 2.0)]);
        let full = SparseDistanceMatrix::from_triplets(7, &full_edges).unwrap();
        assert_eq!(
            recovered.certificate().diagram().bars,
            rips_persistence_sparse(&full, &RipsParams::new(2).with_modulus(5))
                .unwrap()
                .bars
        );
    }

    #[test]
    fn resumes_an_arbitrary_durable_prefix() {
        let temporary = TestStore::new("prefix");
        let store = DurableInterfaceStore::open(&temporary.0).unwrap();
        let certificates = [
            shard(&[0, 1, 2, 3, 4], 0, 5),
            shard(&[0, 1, 2, 3, 5], 1, 5),
            shard(&[0, 1, 2, 3, 6], 2, 5),
        ];
        let artifacts: Vec<_> = certificates
            .iter()
            .map(|item| item.encode(CertificateLimits::default()).unwrap())
            .collect();
        let ids: Vec<_> = artifacts
            .iter()
            .map(|bytes| store.put(bytes).unwrap().0)
            .collect();
        let prefix = RelativeInterfaceCertificate::compose(
            &[&certificates[0], &certificates[1]],
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap();
        let prefix_bytes = prefix.encode(CertificateLimits::default()).unwrap();
        let prefix_id = store.put(&prefix_bytes).unwrap().0;
        let job = job_id(2, 5, &[0, 1, 2, 3], &[], &ids);
        store
            .write_progress(
                job,
                &Progress {
                    prefix: 2,
                    accumulator: prefix_id,
                    folds: vec![ids[0], prefix_id],
                },
            )
            .unwrap();

        let commit = store
            .commit_stored(&ids, &[0, 1, 2, 3], &[], CertificateLimits::default())
            .unwrap();
        assert_eq!(commit.work().folds_reused, 2);
        assert_eq!(commit.work().folds_computed, 1);
        assert_eq!(commit.manifest().folds()[1], prefix_id);
        store
            .verify_manifest(commit.manifest(), CertificateLimits::default())
            .unwrap();
    }

    #[test]
    fn rejects_corrupt_objects_and_incompatible_shards() {
        let temporary = TestStore::new("corrupt");
        let store = DurableInterfaceStore::open(&temporary.0).unwrap();
        let first = shard(&[0, 1, 2, 3, 4], 0, 2)
            .encode(CertificateLimits::default())
            .unwrap();
        let incompatible = shard(&[0, 1, 2, 3, 5], 1, 3)
            .encode(CertificateLimits::default())
            .unwrap();
        assert!(
            store
                .commit(
                    &[first.clone(), incompatible],
                    &[0, 1, 2, 3],
                    &[],
                    CertificateLimits::default(),
                )
                .is_err()
        );
        let (id, _) = store.put(&first).unwrap();
        fs::write(store.object_path(id), b"corrupt").unwrap();
        assert!(
            store
                .get(id, CertificateLimits::default().max_bytes)
                .is_err()
        );
    }
}
