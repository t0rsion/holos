use crate::{ProofError, ProofLimits};

const MAGIC: &[u8; 8] = b"HOLOSCI\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_SCENARIOS: usize = 256;
const FORMAT_MAX_CANDIDATES: usize = 4_096;
const FORMAT_MAX_PROOF_TERMS: usize = 1_000_000;
const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;

mod model;
mod search;
mod verification;
mod wire;

/// Return true when bytes start with the cohomology-intervention magic.
pub fn is_cohomology_intervention(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub use model::{VerifiedCohomologyIntervention, VerifiedCohomologyInterventionStatus};

/// Verify a `HOLOSCI` artifact.
///
/// The checker reconstructs every scenario cohomology space and
/// restriction map. It repeats the weighted antitone search and its
/// lower-bound packing.
pub fn verify_cohomology_intervention(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCohomologyIntervention, ProofError> {
    let claim = wire::decode_claim(bytes, limits)?;
    let checked = verification::run_independent_search(&claim, limits)?;
    verification::verify_search_result(&claim, &checked)?;
    Ok(verification::intervention_summary(&claim, &checked))
}
