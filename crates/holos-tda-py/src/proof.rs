//! Persistence proof and certificate bindings.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use super::common::*;

#[pyfunction]
#[pyo3(signature = (n, initial, updates, threshold=None, modulus=2, threads=1))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_sparse_proof(
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
        ProofArtifact::build(&initial, &updates, &params, CertificateLimits::default())
            .map_err(display_err)?
            .encode()
            .map_err(display_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, protected=Vec::new(), max_dim=1, threshold=None, modulus=2))]
pub(crate) fn compile_relative_interface(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    protected: Vec<usize>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> PyResult<RelativeInterfaceRecord> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let mut params = RipsParams::new(max_dim).with_modulus(modulus);
        params.threshold = threshold;
        let certificate = RelativeInterfaceCertificate::build(
            &input,
            &params,
            &protected,
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let artifact = certificate
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let work = certificate.work();
        Ok((
            artifact,
            to_bars(certificate.diagram().clone()),
            (work.input_cells, work.cancellations, work.core_cells),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, threads=4, score="columns", adaptive_objective="h1", adaptive_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_collapse_portfolio(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    threads: usize,
    score: &str,
    adaptive_objective: &str,
    adaptive_work_limit: Option<u64>,
) -> PyResult<PortfolioRecord> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let adaptive_objective = parse_collapse_objective(adaptive_objective)?;
        let candidates = [
            CollapsePortfolioCandidate::Serial,
            CollapsePortfolioCandidate::Rounds { threads },
            CollapsePortfolioCandidate::Adaptive {
                objective: adaptive_objective,
                work_limit: adaptive_work_limit,
            },
        ];
        let objective = match score {
            "edges" => CollapsePortfolioObjective::Edges,
            "columns" => CollapsePortfolioObjective::ReductionColumns {
                max_homology_dimension: max_dim,
            },
            value => {
                return Err(PyValueError::new_err(format!(
                    "score must be edges or columns, not {value}"
                )));
            }
        };
        let limits = CollapsePortfolioLimits::default().with_max_homology_dimension(max_dim);
        let portfolio =
            collapse_sparse_portfolio(&input, threshold, &candidates, objective, limits)
                .map_err(to_err)?;
        let artifact =
            CollapsePortfolioArtifact::from_portfolio(&portfolio, limits).map_err(to_err)?;
        let bytes = artifact
            .encode(limits, CollapsePortfolioDecodeLimits::default())
            .map_err(to_err)?;
        let entries = portfolio
            .entries()
            .iter()
            .map(|entry| {
                (
                    collapse_candidate_name(entry.candidate()).to_owned(),
                    entry.score().simplex_counts().to_vec(),
                    entry.result().matrix.num_edges(),
                )
            })
            .collect();
        Ok((bytes, portfolio.selected_index(), entries))
    })
}

pub(crate) fn collapse_candidate_name(candidate: CollapsePortfolioCandidate) -> &'static str {
    match candidate {
        CollapsePortfolioCandidate::Serial => "serial",
        CollapsePortfolioCandidate::Rounds { .. } => "rounds",
        CollapsePortfolioCandidate::Adaptive { .. } => "adaptive",
        _ => "other",
    }
}

#[pyfunction]
#[pyo3(signature = (simplices, max_dim=1, modulus=2))]
pub(crate) fn compile_explicit_persistence(
    py: Python<'_>,
    simplices: Vec<(Vec<usize>, f64)>,
    max_dim: usize,
    modulus: u32,
) -> PyResult<ExplicitRecord> {
    py.detach(|| {
        let complex = explicit_complex(simplices, max_dim)?;
        let certificate = ExplicitReductionCertificate::build(
            &complex,
            max_dim,
            modulus,
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let bytes = certificate
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let simplex_counts = certificate
            .complex()
            .simplices()
            .iter()
            .map(Vec::len)
            .collect();
        let column_counts = certificate.columns().iter().map(Vec::len).collect();
        Ok((
            bytes,
            to_bars(certificate.diagram().clone()),
            simplex_counts,
            column_counts,
        ))
    })
}

pub(crate) fn explicit_complex(
    simplices: Vec<(Vec<usize>, f64)>,
    max_dim: usize,
) -> PyResult<FilteredSimplicialComplex<ScalarGrade>> {
    let mut groups = vec![Vec::new(); max_dim.saturating_add(2)];
    for (vertices, grade) in simplices {
        if vertices.is_empty() {
            return Err(PyValueError::new_err("an explicit simplex cannot be empty"));
        }
        let dimension = vertices.len() - 1;
        if dimension >= groups.len() {
            groups.resize_with(dimension + 1, Vec::new);
        }
        let grade = ScalarGrade::new(grade).map_err(display_err)?;
        groups[dimension].push(FilteredSimplex::new(vertices, grade));
    }
    let mut labels = groups[0]
        .iter()
        .filter_map(|simplex| simplex.vertices().first().copied())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();
    FilteredSimplicialComplex::new(labels, groups).map_err(display_err)
}

#[pyfunction]
#[pyo3(signature = (artifacts, store, separator=Vec::new(), protected=Vec::new()))]
pub(crate) fn merge_relative_interfaces(
    py: Python<'_>,
    artifacts: Vec<Vec<u8>>,
    store: String,
    separator: Vec<usize>,
    protected: Vec<usize>,
) -> PyResult<DistributedInterfaceRecord> {
    py.detach(|| {
        let store = DurableInterfaceStore::open(store).map_err(display_err)?;
        let commit = store
            .commit(
                &artifacts,
                &separator,
                &protected,
                CertificateLimits::default(),
            )
            .map_err(display_err)?;
        let manifest = commit.manifest().encode().map_err(display_err)?;
        let result = commit
            .certificate()
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let work = commit.work();
        Ok((
            manifest,
            result,
            commit.manifest().job().to_string(),
            (
                work.shards,
                work.folds_reused,
                work.folds_computed,
                work.peak_artifact_bytes,
            ),
        ))
    })
}
