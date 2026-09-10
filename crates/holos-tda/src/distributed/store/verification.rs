use crate::{CertificateLimits, RelativeInterfaceCertificate};

use super::super::compose::{
    check_fold_id, combined_vertices, compose_certificates, decode_certificate, encode_certificate,
    require_compatible, require_separator,
};
use super::super::model::{
    ArtifactId, DistributedInterfaceError, DistributedInterfaceManifest, DurableInterfaceStore,
};

impl DurableInterfaceStore {
    /// Recompute every fold and verify a committed manifest from stored shards.
    ///
    /// Ignores durable progress. Checks that every recorded fold and the
    /// final result follow from the ordered shard artifacts.
    pub fn verify_manifest(
        &self,
        manifest: &DistributedInterfaceManifest,
        limits: CertificateLimits,
    ) -> Result<RelativeInterfaceCertificate, DistributedInterfaceError> {
        let mut accumulator = self.load_manifest_start(manifest, limits)?;
        self.replay_manifest_folds(manifest, limits, &mut accumulator)?;
        if accumulator.protected_vertices() != manifest.output_protected_vertices {
            accumulator =
                compose_certificates(&[&accumulator], &manifest.output_protected_vertices, limits)?;
        }
        self.check_manifest_result(manifest, &accumulator, limits)?;
        Ok(accumulator)
    }

    fn load_manifest_start(
        &self,
        manifest: &DistributedInterfaceManifest,
        limits: CertificateLimits,
    ) -> Result<RelativeInterfaceCertificate, DistributedInterfaceError> {
        let bytes = self.get(manifest.shards[0], limits.max_bytes)?;
        let accumulator = decode_certificate(&bytes, limits)?;
        require_separator(&accumulator, &manifest.separator_vertices)?;
        check_fold_id(&bytes, manifest.folds[0])?;
        Ok(accumulator)
    }

    fn replay_manifest_folds(
        &self,
        manifest: &DistributedInterfaceManifest,
        limits: CertificateLimits,
        accumulator: &mut RelativeInterfaceCertificate,
    ) -> Result<(), DistributedInterfaceError> {
        let intermediate = combined_vertices(
            &manifest.separator_vertices,
            &manifest.output_protected_vertices,
        );
        for (position, shard) in manifest.shards.iter().enumerate().skip(1) {
            self.replay_manifest_fold(
                manifest,
                limits,
                &intermediate,
                accumulator,
                position,
                *shard,
            )?;
        }
        Ok(())
    }

    fn replay_manifest_fold(
        &self,
        manifest: &DistributedInterfaceManifest,
        limits: CertificateLimits,
        intermediate: &[usize],
        accumulator: &mut RelativeInterfaceCertificate,
        position: usize,
        shard: ArtifactId,
    ) -> Result<(), DistributedInterfaceError> {
        let bytes = self.get(shard, limits.max_bytes)?;
        let child = decode_certificate(&bytes, limits)?;
        require_compatible(accumulator, &child)?;
        require_separator(&child, &manifest.separator_vertices)?;
        *accumulator = compose_certificates(&[accumulator, &child], intermediate, limits)?;
        check_fold_id(
            &encode_certificate(accumulator, limits)?,
            manifest.folds[position],
        )
    }

    fn check_manifest_result(
        &self,
        manifest: &DistributedInterfaceManifest,
        accumulator: &RelativeInterfaceCertificate,
        limits: CertificateLimits,
    ) -> Result<(), DistributedInterfaceError> {
        let encoded = encode_certificate(accumulator, limits)?;
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
        Ok(())
    }
}
