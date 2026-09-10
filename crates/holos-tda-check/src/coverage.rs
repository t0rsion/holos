//! Independent replay of coverage proofs.

mod evaluate;
mod model;
mod proof;
mod source;
mod verify;
mod wire;
mod wire_header;
mod wire_reader;

pub(crate) use model::CoverageGeometryClaim;
pub use model::{VerifiedCoverage, VerifiedCoverageSource, VerifiedCoverageStatus};
pub(crate) use verify::verify_coverage_with_geometry_claim;
pub use verify::{is_coverage, verify_coverage};

const MAGIC: &[u8; 8] = b"HOLOSCOV";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;
const FORMAT_MAX_STATES: usize = 4_096;
const FORMAT_MAX_ACTIONS: usize = 65_536;
const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;
const FORMAT_MAX_PROOF_DEPTH: usize = 4_096;
