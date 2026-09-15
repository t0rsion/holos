//! Checked circular coordinates from canonical fixed-scale H1 classes.
//!
//! The harmonic construction follows the circular-coordinate method of de
//! Silva, Morozov, and Vejdemo-Johansson. Automatic lifting searches centered
//! representatives of every nonzero scalar multiple of the input field class.
//! It rejects a class when that sufficient search finds no integer cocycle.

mod api;
mod harmonic;
mod lift;
mod model;
#[cfg(test)]
mod tests;

pub(crate) use api::{
    CircularCoordinateFailure, circular_coordinate_with_failure_stage, selected_coordinate,
    validate_params,
};
pub use api::{
    circular_coordinate, circular_coordinate_for_class, circular_coordinate_with_integral_lift,
    cocycle_from_ripser_terms, continue_circular_coordinate,
};
pub(crate) use model::SelectedCircularCoordinate;
pub use model::{
    CircularClassTerm, CircularCoordinate, CircularCoordinateContinuation,
    CircularCoordinateParams, IntegralCocycleTerm,
};
