//! Distance-matrix storage and exact point-cloud graph construction.
//!
//! The public types retain their historical paths. Internal cofacet
//! enumeration stays separate from storage, construction, and tests.

mod cofacet;
mod construction;
mod matrix;
mod point_cloud;
mod sparse;

#[cfg(test)]
mod tests;

pub use matrix::DistanceMatrix;
pub use point_cloud::{PointCloudGraph, PointCloudParams, PointCloudStats, PointCloudStrategy};
pub use sparse::SparseDistanceMatrix;

#[allow(unused_imports)]
pub(crate) use cofacet::Cofacet;
pub(crate) use cofacet::Distances;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use cofacet::counters;
#[cfg(test)]
pub(crate) use matrix::SQUARE_BUILDS;
