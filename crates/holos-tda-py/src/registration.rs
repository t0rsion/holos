//! Python module registration.

use pyo3::prelude::*;

use super::{
    atlas, bipersistence, circular, coverage, index, persistence, point, program, proof,
    proof_checks, synthesis, topology,
};

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_persistence(module)?;
    bipersistence::register(module)?;
    register_programs(module)?;
    register_proofs(module)?;
    register_topology(module)?;
    register_synthesis(module)?;
    register_types(module)?;
    register_metadata(module)
}

fn register_persistence(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_persistence_diagrams(module)?;
    register_persistence_classes(module)
}

fn register_persistence_diagrams(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(persistence::rips_points, module)?)?;
    module.add_function(wrap_pyfunction!(persistence::rips_condensed, module)?)?;
    module.add_function(wrap_pyfunction!(persistence::rips_sparse, module)?)?;
    Ok(())
}

fn register_persistence_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(persistence::rips_points_classes, module)?)?;
    module.add_function(wrap_pyfunction!(
        persistence::rips_condensed_classes,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(persistence::rips_sparse_classes, module)?)?;
    register_circular_coordinates(module)
}

fn register_circular_coordinates(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_circular_diagrams(module)?;
    register_circular_classes(module)
}

fn register_circular_diagrams(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(circular::circular_points, module)?)?;
    module.add_function(wrap_pyfunction!(circular::circular_condensed, module)?)?;
    module.add_function(wrap_pyfunction!(circular::circular_sparse, module)?)?;
    Ok(())
}

fn register_circular_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(circular::circular_points_class, module)?)?;
    module.add_function(wrap_pyfunction!(
        circular::circular_condensed_class,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(circular::circular_sparse_class, module)?)?;
    Ok(())
}

fn register_programs(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_atlas_and_index(module)?;
    register_program_artifacts(module)
}

fn register_atlas_and_index(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(atlas::compile_sparse_atlas, module)?)?;
    module.add_function(wrap_pyfunction!(atlas::load_sparse_atlas, module)?)?;
    module.add_function(wrap_pyfunction!(index::compile_sparse_index, module)?)?;
    Ok(())
}

fn register_program_artifacts(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(program::compile_sparse_program, module)?)?;
    module.add_function(wrap_pyfunction!(program::load_sparse_program, module)?)?;
    module.add_function(wrap_pyfunction!(
        program::compile_sparse_program_trace,
        module
    )?)?;
    Ok(())
}

fn register_proofs(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_proof_builders(module)?;
    register_proof_checkers(module)
}

fn register_proof_builders(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_v07_proof_builders(module)?;
    register_interface_proof_builders(module)
}

fn register_v07_proof_builders(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(proof::compile_collapse_portfolio, module)?)?;
    module.add_function(wrap_pyfunction!(
        proof::compile_explicit_persistence,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(proof::compile_sparse_proof, module)?)?;
    Ok(())
}

fn register_interface_proof_builders(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(proof::compile_relative_interface, module)?)?;
    module.add_function(wrap_pyfunction!(proof::merge_relative_interfaces, module)?)?;
    Ok(())
}

fn register_proof_checkers(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        proof_checks::verify_program_trace,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(proof_checks::verify_intervention, module)?)?;
    Ok(())
}

fn register_topology(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_cohomology(module)?;
    register_applications(module)
}

fn register_cohomology(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(topology::fixed_cohomology, module)?)?;
    module.add_function(wrap_pyfunction!(topology::relate_fixed_cohomology, module)?)?;
    module.add_function(wrap_pyfunction!(topology::affine_events, module)?)?;
    Ok(())
}

fn register_applications(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(topology::kinetic_zigzag, module)?)?;
    module.add_function(wrap_pyfunction!(
        topology::intervene_fixed_cohomology,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(coverage::check_relative_coverage, module)?)?;
    Ok(())
}

fn register_synthesis(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register_coverage_synthesis(module)?;
    register_cohomology_synthesis(module)
}

fn register_coverage_synthesis(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        coverage::synthesize_finite_coverage,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        coverage::synthesize_geometric_coverage,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        coverage::synthesize_affine_coverage,
        module
    )?)?;
    Ok(())
}

fn register_cohomology_synthesis(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        synthesis::synthesize_fixed_cohomology,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(
        synthesis::synthesize_affine_cohomology,
        module
    )?)?;
    Ok(())
}

fn register_types(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(point::compile_points_atlas, module)?)?;
    module.add_class::<atlas::PySparseAtlas>()?;
    module.add_class::<index::PySparseIndex>()?;
    module.add_class::<program::PySparseProgram>()?;
    module.add_class::<point::PyPointAtlas>()?;
    module.add_function(wrap_pyfunction!(run_cli, module)?)?;
    Ok(())
}

#[pyfunction]
fn run_cli(py: Python<'_>, argv: Vec<String>) -> i32 {
    py.detach(|| holos_tda::cli::run_cli(argv))
}

fn register_metadata(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", holos_tda::VERSION)?;
    module.add("GIT_HASH", holos_tda::GIT_HASH)?;
    Ok(())
}
