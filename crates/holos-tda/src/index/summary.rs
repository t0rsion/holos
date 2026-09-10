use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::{
    Bar, ClassCorrespondence, CorrespondenceMode, Diagram, DiagramDelta,
    GradedReductionCertificate, Result, class_correspondences,
};

use super::model::{
    IndexSummary, InterfaceMode, InterfaceNode, InterfaceState, InterfaceSummary, PersistenceIndex,
};

pub(crate) fn node_digest(
    vertices: &[usize],
    edge_positions: &[usize],
    separator: &[usize],
    protected_vertices: &[usize],
    children: &[Arc<InterfaceNode>],
    state: &InterfaceState,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-filtered-interface-v4");
    digest_usizes(&mut hash, vertices);
    digest_usizes(&mut hash, edge_positions);
    digest_usizes(&mut hash, separator);
    digest_usizes(&mut hash, protected_vertices);
    hash.update((children.len() as u64).to_be_bytes());
    for child in children {
        hash.update(child.digest);
    }
    let diagram = digest_interface_state(&mut hash, state);
    digest_diagram(&mut hash, diagram);
    hash.finalize().into()
}

fn digest_interface_state<'a>(hash: &mut Sha256, state: &'a InterfaceState) -> &'a Diagram {
    match state {
        InterfaceState::Relative(relative) => {
            hash.update([4]);
            hash.update(relative.source_digest());
            hash.update(relative.digest());
            relative.diagram()
        }
        InterfaceState::Materialized(reduction) => {
            hash.update([0]);
            hash.update(reduction.graph_digest());
            hash.update((reduction.max_dim() as u64).to_be_bytes());
            hash.update((reduction.graded_columns().len() as u64).to_be_bytes());
            for columns in reduction.graded_columns() {
                digest_columns(hash, columns);
            }
            reduction.diagram()
        }
        InterfaceState::Composed { mode, diagram } => {
            hash.update([match mode {
                InterfaceMode::Relative => unreachable!("a composed state has a composed mode"),
                InterfaceMode::Materialized => unreachable!("a composed state has a composed mode"),
                InterfaceMode::Disjoint => 1,
                InterfaceMode::ZeroSimplex => 2,
                InterfaceMode::ZeroCone => 3,
            }]);
            diagram
        }
    }
}

fn digest_diagram(hash: &mut Sha256, diagram: &Diagram) {
    hash.update((diagram.bars.len() as u64).to_be_bytes());
    for bar in &diagram.bars {
        hash.update((bar.dim as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for &value in values {
        hash.update((value as u64).to_be_bytes());
    }
}

fn digest_columns(hash: &mut Sha256, columns: &[crate::ChangeColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}

pub(crate) fn summarize(node: &Arc<InterfaceNode>, summary: &mut IndexSummary) {
    summary.nodes += 1;
    match node.mode() {
        InterfaceMode::Relative => {
            let relative = node.relative().expect("relative mode has a relative core");
            let work = relative.work();
            summary.relative_interfaces += 1;
            summary.composed_interfaces += usize::from(!node.children.is_empty());
            summary.relative_input_cells += work.input_cells;
            summary.relative_core_cells += work.core_cells;
            summary.relative_cancellations += work.cancellations;
            summary.largest_relative_core_cells =
                summary.largest_relative_core_cells.max(work.core_cells);
        }
        InterfaceMode::Materialized => {
            summary.materialized_interfaces += 1;
            summary.largest_interface_vertices =
                summary.largest_interface_vertices.max(node.vertices.len());
            summary.largest_interface_edges = summary
                .largest_interface_edges
                .max(node.edge_positions.len());
        }
        InterfaceMode::Disjoint | InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone => {
            summary.composed_interfaces += 1;
        }
    }
    if node.children.is_empty() {
        summary.leaves += 1;
    } else if node.separator.is_empty() {
        summary.component_splits += 1;
    } else {
        summary.separators += 1;
        summary.widest_separator = summary.widest_separator.max(node.separator.len());
    }
    for child in &node.children {
        summarize(child, summary);
    }
}

pub(crate) fn collect_interfaces(
    node: &Arc<InterfaceNode>,
    depth: usize,
    output: &mut Vec<InterfaceSummary>,
) {
    output.push(InterfaceSummary {
        digest: node.digest,
        depth,
        vertices: node.vertices.clone(),
        separator: node.separator.clone(),
        protected_vertices: node.protected_vertices.clone(),
        edges: node.edge_positions.len(),
        children: node.children.len(),
        mode: node.mode(),
        reduction_columns: node
            .reduction()
            .map(GradedReductionCertificate::column_count)
            .unwrap_or(0),
        columns_by_dimension: node
            .reduction()
            .map(|reduction| reduction.graded_columns().iter().map(Vec::len).collect())
            .unwrap_or_default(),
        relative_input_cells: node
            .relative()
            .map(|value| value.work().input_cells)
            .unwrap_or(0),
        relative_core_cells: node
            .relative()
            .map(|value| value.work().core_cells)
            .unwrap_or(0),
        relative_cancellations: node
            .relative()
            .map(|value| value.work().cancellations)
            .unwrap_or(0),
    });
    for child in &node.children {
        collect_interfaces(child, depth + 1, output);
    }
}

pub(crate) fn count_nodes(node: &Arc<InterfaceNode>) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

pub(crate) fn count_columns(node: &Arc<InterfaceNode>) -> usize {
    node.reduction()
        .map(GradedReductionCertificate::column_count)
        .or_else(|| {
            node.relative()
                .map(|relative| relative.graded_columns().iter().map(Vec::len).sum())
        })
        .unwrap_or(0)
        + node.children.iter().map(count_columns).sum::<usize>()
}

pub(crate) fn shared_nodes(left: &Arc<InterfaceNode>, right: &Arc<InterfaceNode>) -> usize {
    if Arc::ptr_eq(left, right) {
        return count_nodes(left);
    }
    left.children
        .iter()
        .zip(&right.children)
        .map(|(a, b)| shared_nodes(a, b))
        .sum()
}

pub(crate) fn correspondences(
    old: &PersistenceIndex,
    new: &PersistenceIndex,
    mode: CorrespondenceMode,
) -> Result<Vec<ClassCorrespondence>> {
    if mode == CorrespondenceMode::Omit {
        return Ok(Vec::new());
    }
    let old_explained = old.explain()?;
    let new_explained = new.explain()?;
    class_correspondences(
        &old.graph,
        &old_explained.spaces,
        &new.graph,
        &new_explained.spaces,
        old.params.modulus,
    )
}

pub(crate) fn diagram_delta(old: &Diagram, new: &Diagram) -> DiagramDelta {
    type Key = (usize, u64, u64);
    let mut old_counts = BTreeMap::<Key, usize>::new();
    let mut new_counts = BTreeMap::<Key, usize>::new();
    for bar in &old.bars {
        *old_counts
            .entry((bar.dim, bar.birth.to_bits(), bar.death.to_bits()))
            .or_default() += 1;
    }
    for bar in &new.bars {
        *new_counts
            .entry((bar.dim, bar.birth.to_bits(), bar.death.to_bits()))
            .or_default() += 1;
    }
    let keys: BTreeSet<_> = old_counts
        .keys()
        .chain(new_counts.keys())
        .copied()
        .collect();
    let mut delta = DiagramDelta::default();
    for (dim, birth, death) in keys {
        let old_count = old_counts.get(&(dim, birth, death)).copied().unwrap_or(0);
        let new_count = new_counts.get(&(dim, birth, death)).copied().unwrap_or(0);
        delta.removed.extend((new_count..old_count).map(|_| Bar {
            dim,
            birth: f64::from_bits(birth),
            death: f64::from_bits(death),
        }));
        delta.added.extend((old_count..new_count).map(|_| Bar {
            dim,
            birth: f64::from_bits(birth),
            death: f64::from_bits(death),
        }));
    }
    delta
}
