use std::fmt;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::{CertificateLimits, RelativeInterfaceCertificate};

/// Failure during durable interface execution or recovery.
#[derive(Debug)]
pub struct DistributedInterfaceError {
    message: String,
}

impl DistributedInterfaceError {
    pub(super) fn new(message: impl Into<String>) -> Self {
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
pub struct ArtifactId(pub(super) [u8; 32]);

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
    /// Peak byte count of one accumulator and one shard.
    pub peak_artifact_bytes: usize,
}

/// Commit record for one distributed interface result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributedInterfaceManifest {
    pub(super) job: ArtifactId,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) separator_vertices: Vec<usize>,
    pub(super) output_protected_vertices: Vec<usize>,
    pub(super) shards: Vec<ArtifactId>,
    pub(super) folds: Vec<ArtifactId>,
    pub(super) result: ArtifactId,
}

/// Completed distributed composition and its durable manifest.
#[derive(Debug, Clone)]
pub struct DistributedInterfaceCommit {
    pub(super) manifest: DistributedInterfaceManifest,
    pub(super) certificate: RelativeInterfaceCertificate,
    pub(super) work: DistributedInterfaceWork,
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
    pub(super) root: PathBuf,
}

pub(super) struct CommitPlan<'a> {
    pub(super) job: ArtifactId,
    pub(super) shard_ids: &'a [ArtifactId],
    pub(super) separator_vertices: Vec<usize>,
    pub(super) output_protected_vertices: Vec<usize>,
    pub(super) intermediate_protected: Vec<usize>,
    pub(super) limits: CertificateLimits,
}

pub(super) struct FoldState {
    pub(super) accumulator: RelativeInterfaceCertificate,
    pub(super) folds: Vec<ArtifactId>,
    pub(super) next: usize,
    pub(super) accumulator_bytes: Vec<u8>,
}

pub(super) struct PreparedCommit<'a> {
    pub(super) plan: CommitPlan<'a>,
    pub(super) first_id: ArtifactId,
    pub(super) first: RelativeInterfaceCertificate,
}

pub(super) struct Progress {
    pub(super) prefix: usize,
    pub(super) accumulator: ArtifactId,
    pub(super) folds: Vec<ArtifactId>,
}
