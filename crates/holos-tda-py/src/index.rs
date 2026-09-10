//! Sparse persistence index bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use super::common::*;

/// Immutable persistence over filtered separator interfaces.
#[pyclass(name = "SparseIndex")]
pub(crate) struct PySparseIndex {
    index: PersistenceIndex,
}

#[pymethods]
impl PySparseIndex {
    /// Canonical `HOLOSIP` bytes for the current version.
    #[getter]
    fn snapshot<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let proof = IndexSnapshotProof::from_index(&self.index).map_err(display_err)?;
        let bytes = proof.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Content identifier of the current root.
    #[getter]
    fn version(&self) -> String {
        digest_string(&self.index.version())
    }

    /// Highest homology dimension maintained by this index.
    #[getter]
    fn max_dim(&self) -> usize {
        self.index.params().max_dim
    }

    /// Exact current diagram through the configured homology dimension.
    fn result(&self, py: Python<'_>) -> Bars {
        py.detach(|| to_bars(self.index.diagram().clone()))
    }

    /// Compute canonical H1 class spaces on demand.
    fn explain(&self, py: Python<'_>) -> PyResult<ProgramResult> {
        py.detach(|| self.index.explain().map(to_program_result).map_err(to_err))
    }

    /// Structural size of the compiled interface tree.
    fn summary(&self) -> IndexSummaryRecord {
        let summary = self.index.summary();
        (
            (
                summary.nodes,
                summary.leaves,
                summary.separators,
                summary.component_splits,
                summary.widest_separator,
                summary.largest_interface_vertices,
                summary.largest_interface_edges,
                summary.composed_interfaces,
                summary.materialized_interfaces,
            ),
            (
                summary.relative_interfaces,
                summary.relative_input_cells,
                summary.relative_core_cells,
                summary.largest_relative_core_cells,
                summary.relative_cancellations,
            ),
            (
                summary.root_composed,
                summary.separator_candidates_checked,
                summary.separator_search_complete,
            ),
        )
    }

    /// Composed and materialized interfaces in deterministic preorder.
    fn interfaces(&self) -> Vec<InterfaceRecord> {
        self.index
            .interfaces()
            .into_iter()
            .map(|interface| {
                (
                    digest_string(&interface.digest),
                    interface.depth,
                    interface.vertices,
                    interface.separator,
                    interface.protected_vertices,
                    interface.edges,
                    interface.children,
                    match interface.mode {
                        InterfaceMode::Relative => "relative",
                        InterfaceMode::Materialized => "materialized",
                        InterfaceMode::Disjoint => "disjoint",
                        InterfaceMode::ZeroSimplex => "zero_simplex",
                        InterfaceMode::ZeroCone => "zero_cone",
                    }
                    .into(),
                    interface.reduction_columns,
                    interface.columns_by_dimension,
                    (
                        interface.relative_input_cells,
                        interface.relative_core_cells,
                        interface.relative_cancellations,
                    ),
                )
            })
            .collect()
    }

    /// Install an exact next version.
    #[pyo3(signature = (n, triplets, correspondence=true))]
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
        correspondence: bool,
    ) -> PyResult<IndexUpdateRecord> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let old = self.index.clone();
            let transition = old.transition_with(&input, mode).map_err(to_err)?;
            let record = index_update_record(&old, &transition)?;
            self.index = transition.index;
            Ok(record)
        })
    }

    /// Apply an atomic active-topology patch inside the edge envelope.
    #[pyo3(signature = (edits, correspondence=true))]
    fn patch(
        &mut self,
        py: Python<'_>,
        edits: Vec<(String, usize, usize, Option<f64>)>,
        correspondence: bool,
    ) -> PyResult<IndexUpdateRecord> {
        py.detach(|| {
            let edits = edits
                .into_iter()
                .map(|(kind, u, v, value)| match (kind.as_str(), value) {
                    ("set", Some(value)) => Ok(IndexEdit::set_weight(u, v, value)),
                    ("activate", Some(value)) => Ok(IndexEdit::activate(u, v, value)),
                    ("deactivate", None) => Ok(IndexEdit::deactivate(u, v)),
                    ("set" | "activate", None) => {
                        Err(PyValueError::new_err(format!("{kind} requires a value")))
                    }
                    ("deactivate", Some(_)) => {
                        Err(PyValueError::new_err("deactivate does not accept a value"))
                    }
                    _ => Err(PyValueError::new_err(format!(
                        "patch kind must be set, activate, or deactivate, not {kind}"
                    ))),
                })
                .collect::<PyResult<Vec<_>>>()?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let old = self.index.clone();
            let patch = TopologyPatch::new(edits);
            let transition = old.transition_patch_with(&patch, mode).map_err(to_err)?;
            let record = index_update_record(&old, &transition)?;
            self.index = transition.index;
            Ok(record)
        })
    }

    /// Apply an ordered version batch atomically.
    #[pyo3(signature = (n, updates, correspondence=true))]
    fn update_many(
        &mut self,
        py: Python<'_>,
        n: usize,
        updates: Vec<Vec<(usize, usize, f64)>>,
        correspondence: bool,
    ) -> PyResult<Vec<IndexUpdateRecord>> {
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
            let mut candidate = self.index.clone();
            let mut records = Vec::with_capacity(updates.len());
            for input in &updates {
                let transition = candidate.transition_with(input, mode).map_err(to_err)?;
                records.push(index_update_record(&candidate, &transition)?);
                candidate = transition.index;
            }
            self.index = candidate;
            Ok(records)
        })
    }

    /// Advance independent alternatives without changing this version.
    fn fork(
        &self,
        py: Python<'_>,
        n: usize,
        alternatives: Vec<Vec<(usize, usize, f64)>>,
    ) -> PyResult<Vec<(IndexUpdateRecord, Py<PySparseIndex>)>> {
        let branches = py.detach(|| {
            let alternatives = alternatives
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            self.index.branch(&alternatives).map_err(to_err)
        })?;
        branches
            .into_iter()
            .map(|branch| {
                let record = index_update_record(&self.index, &branch.transition)?;
                let index = branch.transition.index;
                Ok((record, Py::new(py, PySparseIndex { index })?))
            })
            .collect()
    }

    /// Compare roots, sharing, and exact diagrams with another version.
    fn diff(&self, other: &PySparseIndex) -> IndexDiffRecord {
        let diff = self.index.diff(&other.index);
        (
            diff.same_envelope,
            diff.shared_nodes,
            bars_from(diff.diagram.removed),
            bars_from(diff.diagram.added),
        )
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, separator_width=4, separator_search_limit=100_000, leaf_vertices=4, interface_policy="relative"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_sparse_index(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    separator_width: usize,
    separator_search_limit: usize,
    leaf_vertices: usize,
    interface_policy: &str,
) -> PyResult<PySparseIndex> {
    py.detach(|| {
        let params = params(
            max_dim, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let mut index_params = IndexParams::default();
        index_params.max_separator_width = separator_width;
        index_params.separator_search_limit = separator_search_limit;
        index_params.leaf_vertices = leaf_vertices;
        index_params.interface_policy = match interface_policy {
            "relative" => InterfacePolicy::Relative,
            "compose" => InterfacePolicy::Compose,
            "materialize" => InterfacePolicy::Materialize,
            value => {
                return Err(PyValueError::new_err(format!(
                    "interface_policy must be 'relative', 'compose', or 'materialize', got {value:?}"
                )));
            }
        };
        let index =
            PersistenceIndex::compile(&input, &params, index_params, CertificateLimits::default())
                .map_err(to_err)?;
        Ok(PySparseIndex { index })
    })
}
