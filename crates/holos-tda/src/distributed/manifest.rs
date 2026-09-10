use super::compose::{job_id, require_canonical_vertices};
use super::model::{ArtifactId, DistributedInterfaceError, DistributedInterfaceManifest};
use super::wire::{
    MANIFEST_MAGIC, Reader, VERSION, decode_ids, decode_usizes, encode_ids, encode_usizes,
    put_usize,
};

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
        check_manifest_size(bytes.len(), maximum_bytes)?;
        let mut reader = Reader::new(bytes);
        check_manifest_identity(&mut reader)?;
        let manifest = decode_manifest_body(&mut reader)?;
        check_manifest_shape(&manifest, reader.remaining())?;
        check_manifest_binding(&manifest)?;
        Ok(manifest)
    }
}

fn check_manifest_size(actual: usize, limit: usize) -> Result<(), DistributedInterfaceError> {
    if actual > limit {
        return Err(DistributedInterfaceError::new(
            "manifest exceeds the byte limit",
        ));
    }
    Ok(())
}

fn check_manifest_identity(reader: &mut Reader<'_>) -> Result<(), DistributedInterfaceError> {
    if reader.take(8)? != MANIFEST_MAGIC || reader.u16()? != VERSION {
        return Err(DistributedInterfaceError::new(
            "unsupported distributed manifest",
        ));
    }
    Ok(())
}

fn decode_manifest_body(
    reader: &mut Reader<'_>,
) -> Result<DistributedInterfaceManifest, DistributedInterfaceError> {
    Ok(DistributedInterfaceManifest {
        job: ArtifactId(reader.array32()?),
        max_dim: reader.usize()?,
        modulus: reader.u32()?,
        separator_vertices: decode_usizes(reader)?,
        output_protected_vertices: decode_usizes(reader)?,
        shards: decode_ids(reader)?,
        folds: decode_ids(reader)?,
        result: ArtifactId(reader.array32()?),
    })
}

fn check_manifest_shape(
    manifest: &DistributedInterfaceManifest,
    remaining: usize,
) -> Result<(), DistributedInterfaceError> {
    if remaining != 0 || manifest.shards.is_empty() || manifest.folds.len() != manifest.shards.len()
    {
        return Err(DistributedInterfaceError::new(
            "distributed manifest shape is invalid",
        ));
    }
    require_canonical_vertices(&manifest.separator_vertices)?;
    require_canonical_vertices(&manifest.output_protected_vertices)?;
    Ok(())
}

fn check_manifest_binding(
    manifest: &DistributedInterfaceManifest,
) -> Result<(), DistributedInterfaceError> {
    let expected = job_id(
        manifest.max_dim,
        manifest.modulus,
        &manifest.separator_vertices,
        &manifest.output_protected_vertices,
        &manifest.shards,
    );
    if expected != manifest.job {
        return Err(DistributedInterfaceError::new(
            "distributed manifest binding is invalid",
        ));
    }
    Ok(())
}
