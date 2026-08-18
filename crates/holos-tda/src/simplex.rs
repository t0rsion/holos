//! A simplex in the reduction: a filtration diameter and a combinadic index.
//! The engine unranks the index to recover the vertices on demand.

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Simplex {
    pub(crate) diameter: f64,
    pub(crate) index: u64,
}
