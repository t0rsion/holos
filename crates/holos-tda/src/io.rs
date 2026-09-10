//! Readers for point clouds and for dense or sparse distance matrices.
//! Writers for the diagram output formats.

mod matrices;
mod points;
mod scanner;
#[cfg(test)]
mod tests;
mod window;
mod writer;

pub use matrices::{
    Triplet, parse_condensed, parse_triplets, read_lower_distance_matrix, read_sparse_matrix,
};
pub use points::{parse_point_cloud, read_point_cloud};
pub use writer::{OutputFormat, write_diagram};
