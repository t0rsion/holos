//! A simplex as it travels through the reduction: a filtration diameter and a
//! combinadic index. The engine recovers vertices on demand by unranking the
//! index.

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Simplex {
    pub(crate) diameter: f64,
    pub(crate) index: u64,
}
