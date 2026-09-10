//! Exact filtered chain cores relative to separator subcomplexes.
//!
//! Equal-filtration unit cancellations leave every protected separator cell
//! fixed. Cores with the same labeled separator compose by identifying equal
//! cells. The resulting chain complex retains exact persistence, including
//! separators with nonzero homology.

mod builder;
mod cancellation;
mod chain;
mod composition;
mod digest;
mod model;
mod reduction;
mod validation;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    InterfaceCancellation, InterfaceCell, InterfaceChainTerm, RelativeInterfaceCertificate,
    RelativeInterfaceWork,
};
