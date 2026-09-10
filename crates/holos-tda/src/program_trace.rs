//! Update traces for persistence programs.
//!
//! A `HOLOSDLT` envelope stores the initial graph and program, every
//! updated graph, work and continuation records, and a new program
//! checkpoint only when certified reuse is not possible.

mod artifact;
mod codec;
mod envelope;
mod model;
mod records;
mod replay;
#[cfg(test)]
mod tests;

const MAGIC: &[u8; 8] = b"HOLOSDLT";
const WIRE_VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

pub use model::{
    ProgramTraceArtifact, ProgramTraceDecodeLimits, ProgramTraceError, ProgramTraceStep,
    VerifiedProgramTrace, VerifiedProgramTraceStep,
};
