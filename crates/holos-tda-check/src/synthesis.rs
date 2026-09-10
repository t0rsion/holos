//! Independent checking for synthesis search proofs.

mod model;
mod oracle;
mod proof;
mod verification;
mod wire;
mod wire_reader;
mod wire_source;

pub use model::{VerifiedSynthesis, VerifiedSynthesisSource, VerifiedSynthesisStatus};

/// Return true when bytes start with a synthesis envelope.
pub fn is_synthesis(bytes: &[u8]) -> bool {
    bytes.starts_with(model::MAGIC)
}

/// Verify one bounded `HOLOSSYN` synthesis proof.
pub fn verify_synthesis(
    bytes: &[u8],
    limits: crate::ProofLimits,
) -> Result<VerifiedSynthesis, crate::ProofError> {
    let decoded = wire::decode_synthesis(bytes, limits)?;
    let checked = verification::verify_claim(&decoded, limits)?;
    Ok(verification::synthesis_summary(&decoded, checked))
}
