#[path = "decode.rs"]
mod decode;
#[path = "encode.rs"]
mod encode;
#[path = "reader.rs"]
mod reader;

use crate::{CohomologyClassAtlas, Error, Result};

use super::model::BipersistenceArtifact;
use super::{
    BipersistenceArtifactLimits, BipersistenceArtifactSummary, BipersistenceRectangleClaim,
    BipersistenceRegionClaim,
};

pub(super) fn encode_payload(artifact: &BipersistenceArtifact) -> Result<Vec<u8>> {
    encode::encode_payload(artifact)
}

impl BipersistenceArtifact {
    /// Encode canonical `HOLOSBP` version 2 bytes.
    pub fn encode(&self, limits: BipersistenceArtifactLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        self.encode_after_verification(limits)
    }

    fn encode_after_verification(&self, limits: BipersistenceArtifactLimits) -> Result<Vec<u8>> {
        let mut output = encode_payload(self)?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "bipersistence artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSBP` version 2 bytes.
    pub fn decode(bytes: &[u8], limits: BipersistenceArtifactLimits) -> Result<Self> {
        decode::decode_artifact(bytes, limits)
    }

    /// Structural counts without replaying the artifact.
    pub fn summary(&self) -> BipersistenceArtifactSummary {
        BipersistenceArtifactSummary {
            vertices: self.vertex_count,
            edges: self.edges.len(),
            nodes: self.nodes.len(),
            cover_maps: self.cover_maps.len(),
            rectangles: self.rectangles.len(),
            regions: self.regions.len(),
            class_atlases: self.class_atlases.len(),
            circular_families: self.circular_families.len(),
        }
    }

    /// Stored generalized rectangle-rank claims.
    pub fn rectangles(&self) -> &[BipersistenceRectangleClaim] {
        &self.rectangles
    }

    /// Stored generalized connected-region rank claims.
    pub fn regions(&self) -> &[BipersistenceRegionClaim] {
        &self.regions
    }

    /// Stored class-extension atlases.
    pub fn class_atlases(&self) -> &[CohomologyClassAtlas] {
        &self.class_atlases
    }
}
