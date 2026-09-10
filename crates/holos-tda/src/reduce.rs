//! The serial reduction engine: implicit persistent cohomology following
//! ripser (Bauer 2021). The engine uses the anti-transpose convention,
//! clearing, emergent and apparent pair shortcuts, lazy-cancellation heaps,
//! and on-demand regeneration of reducer columns. Simplices exist only as
//! combinadic indices. The engine never materializes a simplex list of
//! dimension max_dim+1.
//!
//! The engine itself holds no mutable state. A shared kernel takes `&self`
//! plus the buffers its caller owns, so the same methods drive this serial
//! path and the parallel one in [`crate::parallel`], where each worker owns
//! its own buffers. The implementation is split by construction, reduction,
//! pairing, and dimension-zero traversal responsibilities.

mod assembly;
mod coboundary;
mod dim0;
mod engine;
mod model;
mod pairing;
mod reduction;
#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

pub(crate) use model::{
    ApparentPair, Engine, PairScratch, PairTable, Pairing, Pivots, RawH1Class, RawH1Term,
};
