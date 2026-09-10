//! Independent checking for `HOLOSPRG` persistence program artifacts.

mod atlas_wire;
mod claim;
mod class_verify;
mod graph;
mod model;
mod reduction;
mod reduction_wire;
mod trace;
mod trace_compose;
mod trace_correspondence;
mod trace_model;
mod trace_reduction;
mod trace_reduction_replay;
mod trace_region;
mod trace_relation;
mod trace_topology;
mod trace_validation;
mod trace_wire;
mod trace_wire_records;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod trace_tests;

pub use graph::{ProgramEdge, ProgramGraph};
pub use model::{ProgramProofLimits, VerifiedProgram};
pub use trace::{
    ProgramTraceProofLimits, VerifiedProgramTrace, is_program_trace, verify_program_trace,
};

const MAGIC: &[u8; 8] = b"HOLOSPRG";

/// Return true when bytes start with a `HOLOSPRG` program envelope.
pub fn is_program(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one `HOLOSPRG` artifact against its separately supplied graph.
///
/// The checker reconstructs the deterministic decomposition, each nested
/// reduction, the reduction-derived H1 pairs, and the composed H0 and H1
/// diagram. It does not call a persistence solver.
pub fn verify_program(
    bytes: &[u8],
    graph: &ProgramGraph,
    limits: ProgramProofLimits,
) -> Result<VerifiedProgram, crate::ProofError> {
    let claim = wire::decode_program(bytes, limits)?;
    verify::verify_program(&claim, graph, limits)
}
