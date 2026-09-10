//! Filtered complexes and sparse algebraic reduction.

mod columns;
mod complex;
mod repair;

pub(super) use columns::{
    SparseColumn, reduce_with_basis, reduce_with_prefix, valid_reduction_prefix_len,
};
pub(super) use complex::{FilteredComplex, FilteredTriangle};
pub(super) use repair::{DimensionRepair, reindex_change_columns, reindexed_prefix_candidates};
