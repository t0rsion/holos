//! Coverage artifact wire format.

use sha2::{Digest, Sha256};

use crate::{Error, Result};

use super::model::{CoverageSynthesisArtifact, CoverageSynthesisLimits};

mod decode;
mod encode;

const MAGIC: &[u8; 8] = b"HOLOSCOV";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
use decode::{
    Reader, decode_coverage_actions, decode_coverage_prefix, decode_coverage_proof_data,
    decode_coverage_search, decode_coverage_specification, decode_coverage_trailer,
};
use encode::{
    encode_coverage_actions, encode_coverage_prefix, encode_coverage_proof_data,
    encode_coverage_search, encode_coverage_specification,
};

impl CoverageSynthesisArtifact {
    /// Encode canonical `HOLOSCOV` version 1 bytes.
    pub fn encode(&self, limits: CoverageSynthesisLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "coverage artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSCOV` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: CoverageSynthesisLimits) -> Result<Self> {
        validate_coverage_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_coverage_prefix(&mut reader)?;
        let specification = decode_coverage_specification(&mut reader, limits)?;
        let actions = decode_coverage_actions(&mut reader, specification.states.len(), limits)?;
        let search = decode_coverage_search(&mut reader, actions.len())?;
        let proof = decode_coverage_proof_data(&mut reader, actions.len(), limits)?;
        let digest = decode_coverage_trailer(&mut reader)?;
        let artifact = Self {
            specification,
            actions,
            max_activations: search.max_activations,
            oracle_limit: search.oracle_limit,
            node_limit: search.node_limit,
            status: search.status,
            selected: search.selected,
            lower_bound_cost: search.lower_bound_cost,
            upper_bound_cost: search.upper_bound_cost,
            producer_oracle_calls: search.producer_oracle_calls,
            producer_search_nodes: search.producer_search_nodes,
            producer_cache_hits: search.producer_cache_hits,
            root_blockers: proof.root_blockers,
            before: proof.before,
            after: proof.after,
            proof: proof.proof,
            proof_work: proof.work,
            digest,
        };
        validate_decoded_coverage(&artifact, limits)?;
        Ok(artifact)
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        encode_coverage_prefix(&mut output);
        encode_coverage_specification(&mut output, &self.specification)?;
        encode_coverage_actions(&mut output, &self.actions)?;
        encode_coverage_search(&mut output, self)?;
        encode_coverage_proof_data(&mut output, self)?;
        Ok(output)
    }

    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = self.encode_payload()?;
        Ok(Sha256::digest(payload).into())
    }
}

fn validate_coverage_artifact_size(bytes: &[u8], limits: CoverageSynthesisLimits) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "coverage artifact exceeds its byte limit or is truncated".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_decoded_coverage(
    artifact: &CoverageSynthesisArtifact,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    artifact.verify(limits)?;
    if artifact.compute_digest()? != artifact.digest {
        Err(Error::InvalidInput(
            "coverage artifact digest differs from its content".into(),
        ))
    } else {
        Ok(())
    }
}
