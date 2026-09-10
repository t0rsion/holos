//! Sparse persistence program bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use super::common::*;

/// Compositional sparse persistence with checked local updates.
#[pyclass(name = "SparseProgram")]
pub(crate) struct PySparseProgram {
    program: PersistenceProgram,
    artifact: ProgramArtifact,
}

#[pymethods]
impl PySparseProgram {
    /// Canonical `HOLOSPRG` bytes for the current checked program.
    #[getter]
    fn artifact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Canonical `HOLOSPF` bytes for the current checked state.
    #[getter]
    fn proof<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let proof = ProofArtifact::from_program(&self.program).map_err(display_err)?;
        let bytes = proof.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Exact diagram and canonical H1 class spaces at the current graph.
    fn result(&self, py: Python<'_>) -> ProgramResult {
        py.detach(|| to_program_result(self.program.result().clone()))
    }

    /// Structural program size and result-sensitive guard count.
    fn summary(&self) -> ProgramSummaryRecord {
        let summary = self.program.summary();
        (
            summary.atoms,
            summary.cyclic_atoms,
            summary.articulation_vertices,
            summary.zero_simplex_separators,
            summary.widest_separator,
            summary.separator_candidates_checked,
            summary.separator_search_complete,
            summary.largest_cyclic_atom_edges,
            summary.guards,
        )
    }

    /// Articulation-separated atoms in stable program order.
    fn atoms(&self) -> Vec<ProgramAtomRecord> {
        self.program
            .atoms()
            .iter()
            .map(|atom| {
                (
                    atom.id,
                    atom.vertices.clone(),
                    atom.edges.iter().map(|edge| (edge.u, edge.v)).collect(),
                    atom.separator_vertices.clone(),
                    atom.cyclic,
                )
            })
            .collect()
    }

    /// Evaluate a graph inside every touched result-sensitive region.
    fn evaluate(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<(Bars, WorkRecord)> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let evaluation = self.program.evaluate_diagram(&input).map_err(to_err)?;
            Ok((to_bars(evaluation.diagram), work_record(evaluation.work)))
        })
    }

    /// Reuse valid atoms, rebuild invalid atoms, or recompile after topology changes.
    #[pyo3(signature = (n, triplets, correspondence=true))]
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
        correspondence: bool,
    ) -> PyResult<ProgramUpdateRecord> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let update = self.program.advance_with(&input, mode).map_err(to_err)?;
            self.artifact = ProgramArtifact::from_program(&self.program).map_err(display_err)?;
            Ok(program_update_record(update))
        })
    }

    /// Apply an ordered update batch atomically.
    #[pyo3(signature = (n, updates, correspondence=true))]
    fn update_many(
        &mut self,
        py: Python<'_>,
        n: usize,
        updates: Vec<Vec<(usize, usize, f64)>>,
        correspondence: bool,
    ) -> PyResult<Vec<ProgramUpdateRecord>> {
        py.detach(|| {
            let updates = updates
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let results = self
                .program
                .advance_batch_with(&updates, mode)
                .map_err(to_err)?;
            self.artifact = ProgramArtifact::from_program(&self.program).map_err(display_err)?;
            Ok(results.into_iter().map(program_update_record).collect())
        })
    }

    /// Advance independent alternatives without changing this program.
    fn fork(
        &self,
        py: Python<'_>,
        n: usize,
        alternatives: Vec<Vec<(usize, usize, f64)>>,
    ) -> PyResult<Vec<(ProgramUpdateRecord, Py<PySparseProgram>)>> {
        let branches = py.detach(|| {
            let alternatives = alternatives
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            self.program.branch(&alternatives).map_err(to_err)
        })?;
        branches
            .into_iter()
            .map(|branch| {
                let (update, program) = branch.into_parts();
                let update = program_update_record(update);
                let artifact = ProgramArtifact::from_program(&program).map_err(display_err)?;
                Ok((update, Py::new(py, PySparseProgram { program, artifact })?))
            })
            .collect()
    }

    /// Certify one restricted finite H1 lifetime intervention.
    fn intervene(
        &self,
        py: Python<'_>,
        space: usize,
        before: f64,
        budget: usize,
    ) -> PyResult<InterventionRecord> {
        py.detach(|| {
            let target = self.program.result().spaces.get(space).ok_or_else(|| {
                PyValueError::new_err(format!(
                    "H1 class-space index {space} is out of range for {} spaces",
                    self.program.result().spaces.len()
                ))
            })?;
            let result = self
                .program
                .kill_h1_before(target.id, before, InterventionBudget::new(budget))
                .map_err(to_err)?;
            let status = match result.status {
                holos_tda::InterventionStatus::Optimal => "optimal",
                holos_tda::InterventionStatus::BoundedGap => "bounded_gap",
                holos_tda::InterventionStatus::BudgetLimited => "budget_limited",
                _ => "unknown",
            };
            let artifact = result
                .artifact
                .map(|artifact| artifact.encode().map_err(display_err))
                .transpose()?;
            Ok((
                status.into(),
                result.target.to_string(),
                result.lower_bound,
                result.upper_bound,
                result
                    .edits
                    .into_iter()
                    .map(|edit| ((edit.edge.u, edit.edge.v), edit.before, edit.after))
                    .collect(),
                result.result.map(to_program_result),
                artifact,
            ))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, threshold=None, modulus=2, threads=1))]
pub(crate) fn compile_sparse_program(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
) -> PyResult<PySparseProgram> {
    py.detach(|| {
        let params = params(
            1, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let (artifact, program) =
            ProgramArtifact::compile(&input, &params, CertificateLimits::default())
                .map_err(display_err)?;
        Ok(PySparseProgram { program, artifact })
    })
}

#[pyfunction]
pub(crate) fn load_sparse_program(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    artifact: Vec<u8>,
) -> PyResult<PySparseProgram> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let artifact = ProgramArtifact::decode(
            &artifact,
            program_decode_limits(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let program = artifact
            .verify(&input, CertificateLimits::default())
            .map_err(display_err)?;
        Ok(PySparseProgram { program, artifact })
    })
}

#[pyfunction]
#[pyo3(signature = (n, initial, updates, threshold=None, modulus=2, threads=1))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_sparse_program_trace(
    py: Python<'_>,
    n: usize,
    initial: Vec<(usize, usize, f64)>,
    updates: Vec<Vec<(usize, usize, f64)>>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
) -> PyResult<Vec<u8>> {
    py.detach(|| {
        let params = params(
            1, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let initial = SparseDistanceMatrix::from_triplets(n, &initial).map_err(to_err)?;
        let updates = updates
            .into_iter()
            .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
            .collect::<PyResult<Vec<_>>>()?;
        ProgramTraceArtifact::build(&initial, &updates, &params, CertificateLimits::default())
            .map_err(display_err)?
            .encode()
            .map_err(display_err)
    })
}
