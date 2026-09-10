use std::sync::Arc;

use crate::{CertificateLimits, EdgeKey, Result, RipsParams, SparseDistanceMatrix};

use super::model::{IndexParams, IndexSummary, InterfaceMode, PersistenceIndex};
use super::summary::summarize;

mod decompose;
mod interface;

pub(crate) use interface::{compose_diagram, composition_mode, local_graph};

impl PersistenceIndex {
    /// Compile a separator-tree index through `params.max_dim`.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        index_params: IndexParams,
        limits: CertificateLimits,
    ) -> Result<Self> {
        decompose::validate_params(params, index_params)?;
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let scope = Scope {
            vertices: (0..input.len()).collect(),
            edge_positions: (0..topology.len()).collect(),
        };
        let mut search = decompose::SeparatorSearch::new(&topology, index_params);
        let tree = search.decompose(scope);
        let root = interface::compile_node(
            &tree,
            input,
            &topology,
            params,
            index_params.interface_policy,
            limits,
            &[],
        )?;
        let mut summary = IndexSummary {
            max_dim: params.max_dim,
            separator_candidates_checked: search.checked,
            separator_search_complete: search.complete,
            ..IndexSummary::default()
        };
        summarize(&root, &mut summary);
        summary.root_composed = root.mode() != InterfaceMode::Materialized;
        Ok(Self {
            params: params.clone(),
            index_params,
            limits,
            graph: Arc::new(input.clone()),
            topology: Arc::new(topology),
            root,
            summary,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Scope {
    pub(crate) vertices: Vec<usize>,
    pub(crate) edge_positions: Vec<usize>,
}
