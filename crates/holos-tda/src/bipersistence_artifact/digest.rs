use sha2::{Digest, Sha256};

use crate::Result;

use super::model::BipersistenceArtifact;

impl BipersistenceArtifact {
    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = super::wire::encode_payload(self)?;
        let mut hash = Sha256::new();
        hash.update(b"holos-bipersistence-v2");
        hash.update(payload);
        Ok(hash.finalize().into())
    }
}
