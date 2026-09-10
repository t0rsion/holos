use std::fs;
use std::path::{Path, PathBuf};

use crate::{CertificateLimits, RelativeInterfaceCertificate};

use super::compose::{
    canonical_vertices, check_progress_identity, check_progress_prefix, check_progress_shape,
    combined_vertices, compose_accumulator, compose_certificates, decode_certificate,
    encode_certificate, job_id, require_first_shard, require_separator,
};
use super::fs::{atomic_replace, atomic_write, read_bounded, sync_directory, sync_parent};
use super::model::{
    ArtifactId, CommitPlan, DistributedInterfaceCommit, DistributedInterfaceError,
    DistributedInterfaceManifest, DistributedInterfaceWork, DurableInterfaceStore, FoldState,
    PreparedCommit, Progress,
};
use super::wire::{PROGRESS_MAGIC, Reader, VERSION, decode_ids, encode_ids, put_usize};

mod verification;

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

    /// Compose shard artifacts and publish one atomic manifest.
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
    /// for each fold. The identifiers are ordered and bind the job.
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
        let prepared = self.prepare_commit(
            shard_ids,
            separator_vertices,
            output_protected_vertices,
            limits,
            &mut work,
        )?;
        if let Some(manifest) = self.read_manifest(prepared.plan.job, limits.max_bytes)? {
            return self.load_commit(manifest, work, limits);
        }
        let mut state =
            self.resume_or_start(&prepared.plan, prepared.first_id, prepared.first, &mut work)?;
        self.compute_folds(&prepared.plan, &mut state, &mut work)?;
        self.finish_protection(&prepared.plan, &mut state, &mut work)?;
        self.publish_commit(&prepared.plan, state, work)
    }

    fn prepare_commit<'a>(
        &self,
        shard_ids: &'a [ArtifactId],
        separator_vertices: &[usize],
        output_protected_vertices: &[usize],
        limits: CertificateLimits,
        work: &mut DistributedInterfaceWork,
    ) -> Result<PreparedCommit<'a>, DistributedInterfaceError> {
        let first_id = require_first_shard(shard_ids)?;
        let separator_vertices = canonical_vertices(separator_vertices)?;
        let output_protected_vertices = canonical_vertices(output_protected_vertices)?;
        let first_bytes = self.get(first_id, limits.max_bytes)?;
        work.bytes_read += first_bytes.len();
        work.peak_artifact_bytes = first_bytes.len();
        work.shards_loaded += 1;
        let first = decode_certificate(&first_bytes, limits)?;
        require_separator(&first, &separator_vertices)?;
        let plan = CommitPlan {
            job: job_id(
                first.max_dim(),
                first.modulus(),
                &separator_vertices,
                &output_protected_vertices,
                shard_ids,
            ),
            shard_ids,
            intermediate_protected: combined_vertices(
                &separator_vertices,
                &output_protected_vertices,
            ),
            separator_vertices,
            output_protected_vertices,
            limits,
        };
        Ok(PreparedCommit {
            plan,
            first_id,
            first,
        })
    }

    fn resume_or_start(
        &self,
        plan: &CommitPlan<'_>,
        first_id: ArtifactId,
        first: RelativeInterfaceCertificate,
        work: &mut DistributedInterfaceWork,
    ) -> Result<FoldState, DistributedInterfaceError> {
        let Some(progress) = self.read_progress(plan.job, plan.limits.max_bytes)? else {
            return self.start_folds(plan, first_id, first);
        };
        check_progress_prefix(progress.prefix, plan.shard_ids.len())?;
        let bytes = self.get(progress.accumulator, plan.limits.max_bytes)?;
        work.bytes_read += bytes.len();
        work.folds_reused = progress.prefix;
        Ok(FoldState {
            accumulator: decode_certificate(&bytes, plan.limits)?,
            folds: progress.folds,
            next: progress.prefix,
            accumulator_bytes: bytes,
        })
    }

    fn start_folds(
        &self,
        plan: &CommitPlan<'_>,
        first_id: ArtifactId,
        first: RelativeInterfaceCertificate,
    ) -> Result<FoldState, DistributedInterfaceError> {
        let folds = vec![first_id];
        self.write_progress(
            plan.job,
            &Progress {
                prefix: 1,
                accumulator: first_id,
                folds: folds.clone(),
            },
        )?;
        Ok(FoldState {
            accumulator_bytes: encode_certificate(&first, plan.limits)?,
            accumulator: first,
            folds,
            next: 1,
        })
    }

    fn compute_folds(
        &self,
        plan: &CommitPlan<'_>,
        state: &mut FoldState,
        work: &mut DistributedInterfaceWork,
    ) -> Result<(), DistributedInterfaceError> {
        for (position, shard_id) in plan.shard_ids.iter().enumerate().skip(state.next) {
            self.compute_one_fold(plan, state, work, position, *shard_id)?;
        }
        Ok(())
    }

    fn compute_one_fold(
        &self,
        plan: &CommitPlan<'_>,
        state: &mut FoldState,
        work: &mut DistributedInterfaceWork,
        position: usize,
        shard_id: ArtifactId,
    ) -> Result<(), DistributedInterfaceError> {
        let (child, child_bytes) = self.load_child(plan, shard_id, work)?;
        compose_accumulator(plan, state, child, child_bytes, work)?;
        let fold = self.store_fold(&state.accumulator_bytes, work)?;
        state.folds.push(fold);
        state.next = position + 1;
        work.folds_computed += 1;
        self.write_progress(
            plan.job,
            &Progress {
                prefix: state.next,
                accumulator: fold,
                folds: state.folds.clone(),
            },
        )
    }

    fn load_child(
        &self,
        plan: &CommitPlan<'_>,
        shard_id: ArtifactId,
        work: &mut DistributedInterfaceWork,
    ) -> Result<(RelativeInterfaceCertificate, usize), DistributedInterfaceError> {
        let bytes = self.get(shard_id, plan.limits.max_bytes)?;
        work.bytes_read += bytes.len();
        work.shards_loaded += 1;
        let child = decode_certificate(&bytes, plan.limits)?;
        require_separator(&child, &plan.separator_vertices)?;
        Ok((child, bytes.len()))
    }

    fn store_fold(
        &self,
        bytes: &[u8],
        work: &mut DistributedInterfaceWork,
    ) -> Result<ArtifactId, DistributedInterfaceError> {
        let (fold, written) = self.put(bytes)?;
        if written {
            work.bytes_written += bytes.len();
        }
        Ok(fold)
    }

    fn finish_protection(
        &self,
        plan: &CommitPlan<'_>,
        state: &mut FoldState,
        work: &mut DistributedInterfaceWork,
    ) -> Result<(), DistributedInterfaceError> {
        if state.accumulator.protected_vertices() == plan.output_protected_vertices {
            return Ok(());
        }
        state.accumulator = compose_certificates(
            &[&state.accumulator],
            &plan.output_protected_vertices,
            plan.limits,
        )?;
        state.accumulator_bytes = encode_certificate(&state.accumulator, plan.limits)?;
        self.store_fold(&state.accumulator_bytes, work)?;
        Ok(())
    }

    fn publish_commit(
        &self,
        plan: &CommitPlan<'_>,
        state: FoldState,
        work: DistributedInterfaceWork,
    ) -> Result<DistributedInterfaceCommit, DistributedInterfaceError> {
        let manifest = DistributedInterfaceManifest {
            job: plan.job,
            max_dim: state.accumulator.max_dim(),
            modulus: state.accumulator.modulus(),
            separator_vertices: plan.separator_vertices.clone(),
            output_protected_vertices: plan.output_protected_vertices.clone(),
            shards: plan.shard_ids.to_vec(),
            folds: state.folds,
            result: ArtifactId::for_bytes(&state.accumulator_bytes),
        };
        atomic_write(&self.manifest_path(plan.job), &manifest.encode()?)?;
        Ok(DistributedInterfaceCommit {
            manifest,
            certificate: state.accumulator,
            work,
        })
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

    pub(super) fn object_path(&self, id: ArtifactId) -> PathBuf {
        let hex = id.to_hex();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    pub(super) fn manifest_path(&self, id: ArtifactId) -> PathBuf {
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

    pub(super) fn write_progress(
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
        check_progress_identity(&mut reader, job)?;
        let prefix = reader.usize()?;
        let accumulator = ArtifactId(reader.array32()?);
        let folds = decode_ids(&mut reader)?;
        check_progress_shape(reader.remaining(), prefix, accumulator, &folds)?;
        Ok(Some(Progress {
            prefix,
            accumulator,
            folds,
        }))
    }
}
