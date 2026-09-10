//! Persistence trajectories across atlas regions.
//!
//! A `HOLOSTRC` record contains the initial graph and atlas, every later
//! graph, each validity-region event, and a new proof at every region
//! boundary. The verifier checks proofs at boundaries and evaluates all
//! other steps from the current atlas without calling the persistence
//! solver.

mod decode;
mod encode;
mod model;
mod primitives;
mod verification;

#[cfg(test)]
mod tests;

pub use model::{
    TrajectoryArtifact, TrajectoryDecodeLimits, TrajectoryError, TrajectoryStep,
    VerifiedTrajectory, VerifiedTrajectoryStep,
};

const MAGIC: &[u8; 8] = b"HOLOSTRC";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
