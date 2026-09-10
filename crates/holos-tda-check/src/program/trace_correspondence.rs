use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::proof::ProofError;

use super::claim::CocycleTermClaim;
use super::graph::ProgramGraph;
use super::trace_model::{
    ProgramTraceProofLimits, ResultSpace, TraceContinuation, TraceContinuationKind,
    TraceCorrespondence, TraceTransport,
};
use super::trace_relation::{image_intersection, independent_image, restricted_rows, terms_for};

pub(super) fn continuations(
    old: &[ResultSpace],
    new: &[ResultSpace],
    max_continuations: usize,
    max_transports: usize,
) -> Result<Vec<TraceContinuation>, ProofError> {
    let old_terms = index_terms(old);
    let new_terms = index_terms(new);
    let node_count = old
        .len()
        .checked_add(new.len())
        .ok_or_else(|| ProofError::new("trace continuation node count overflows usize"))?;
    let graph =
        build_continuation_graph(old_terms, &new_terms, old.len(), node_count, max_transports)?;
    let mut output = connected_continuations(old, new, &graph, max_continuations)?;
    append_unmatched_continuations(&mut output, old, new, &graph.adjacency, max_continuations)?;
    output.sort_by(|left, right| {
        left.old_spaces
            .cmp(&right.old_spaces)
            .then(left.new_spaces.cmp(&right.new_spaces))
    });
    Ok(output)
}

type TermIndex = BTreeMap<Vec<CocycleTermClaim>, Vec<(usize, [u8; 32])>>;

fn index_terms(spaces: &[ResultSpace]) -> TermIndex {
    let mut indexed: TermIndex = BTreeMap::new();
    for (index, space) in spaces.iter().enumerate() {
        for class in &space.basis {
            indexed
                .entry(class.terms.clone())
                .or_default()
                .push((index, class.id));
        }
    }
    indexed
}

struct ContinuationGraph {
    adjacency: Vec<BTreeSet<usize>>,
    transports: BTreeMap<(usize, usize), Vec<TraceTransport>>,
}

fn build_continuation_graph(
    old_terms: TermIndex,
    new_terms: &TermIndex,
    old_count: usize,
    node_count: usize,
    max_transports: usize,
) -> Result<ContinuationGraph, ProofError> {
    let mut adjacency = vec![BTreeSet::new(); node_count];
    let mut transports: BTreeMap<(usize, usize), Vec<TraceTransport>> = BTreeMap::new();
    let mut transport_total = 0;
    for (terms, left) in old_terms {
        let Some(right) = new_terms.get(&terms) else {
            continue;
        };
        for &(old_space, old_id) in &left {
            for &(new_space, new_id) in right {
                adjacency[old_space].insert(old_count + new_space);
                adjacency[old_count + new_space].insert(old_space);
                transport_total =
                    bounded_add(transport_total, 1, max_transports, "trace basis transports")?;
                transports
                    .entry((old_space, new_space))
                    .or_default()
                    .push((old_id, new_id, 1));
            }
        }
    }
    Ok(ContinuationGraph {
        adjacency,
        transports,
    })
}

fn connected_continuations(
    old: &[ResultSpace],
    new: &[ResultSpace],
    graph: &ContinuationGraph,
    max_continuations: usize,
) -> Result<Vec<TraceContinuation>, ProofError> {
    let mut seen = vec![false; graph.adjacency.len()];
    let mut output = Vec::new();
    for start in 0..graph.adjacency.len() {
        if seen[start] || graph.adjacency[start].is_empty() {
            continue;
        }
        let (old_nodes, new_nodes) =
            continuation_component(start, old.len(), &graph.adjacency, &mut seen);
        let continuation = component_continuation(old, new, graph, old_nodes, new_nodes)?;
        push_bounded(
            &mut output,
            continuation,
            max_continuations,
            "trace continuations",
        )?;
    }
    Ok(output)
}

fn component_continuation(
    old: &[ResultSpace],
    new: &[ResultSpace],
    graph: &ContinuationGraph,
    old_nodes: Vec<usize>,
    new_nodes: Vec<usize>,
) -> Result<TraceContinuation, ProofError> {
    let mut transport = Vec::new();
    for &left in &old_nodes {
        for &right in &new_nodes {
            if let Some(values) = graph.transports.get(&(left, right)) {
                transport.extend(values);
            }
        }
    }
    transport.sort_unstable();
    transport.dedup();
    let old_rank = continuation_rank(&old_nodes, old, "old")?;
    let new_rank = continuation_rank(&new_nodes, new, "new")?;
    let complete = transport.len() == old_rank && transport.len() == new_rank;
    Ok(TraceContinuation {
        kind: continuation_kind(old_nodes.len(), new_nodes.len(), complete),
        old_spaces: old_nodes.iter().map(|&node| old[node].id).collect(),
        new_spaces: new_nodes.iter().map(|&node| new[node].id).collect(),
        transport,
    })
}

fn continuation_rank(
    nodes: &[usize],
    spaces: &[ResultSpace],
    label: &str,
) -> Result<usize, ProofError> {
    nodes.iter().try_fold(0usize, |total, &node| {
        total
            .checked_add(spaces[node].basis.len())
            .ok_or_else(|| ProofError::new(format!("{label} continuation rank overflows usize")))
    })
}

fn append_unmatched_continuations(
    output: &mut Vec<TraceContinuation>,
    old: &[ResultSpace],
    new: &[ResultSpace],
    adjacency: &[BTreeSet<usize>],
    max_continuations: usize,
) -> Result<(), ProofError> {
    for (index, space) in old.iter().enumerate() {
        if adjacency[index].is_empty() {
            push_bounded(
                output,
                TraceContinuation {
                    kind: TraceContinuationKind::Death,
                    old_spaces: vec![space.id],
                    new_spaces: Vec::new(),
                    transport: Vec::new(),
                },
                max_continuations,
                "trace continuations",
            )?;
        }
    }
    for (index, space) in new.iter().enumerate() {
        if adjacency[old.len() + index].is_empty() {
            push_bounded(
                output,
                TraceContinuation {
                    kind: TraceContinuationKind::Birth,
                    old_spaces: Vec::new(),
                    new_spaces: vec![space.id],
                    transport: Vec::new(),
                },
                max_continuations,
                "trace continuations",
            )?;
        }
    }
    Ok(())
}

fn continuation_component(
    start: usize,
    old_count: usize,
    adjacency: &[BTreeSet<usize>],
    seen: &mut [bool],
) -> (Vec<usize>, Vec<usize>) {
    let mut queue = VecDeque::from([start]);
    seen[start] = true;
    let mut old_nodes = Vec::new();
    let mut new_nodes = Vec::new();
    while let Some(node) = queue.pop_front() {
        if node < old_count {
            old_nodes.push(node);
        } else {
            new_nodes.push(node - old_count);
        }
        for &next in &adjacency[node] {
            if !seen[next] {
                seen[next] = true;
                queue.push_back(next);
            }
        }
    }
    old_nodes.sort_unstable();
    new_nodes.sort_unstable();
    (old_nodes, new_nodes)
}

fn continuation_kind(old: usize, new: usize, complete: bool) -> TraceContinuationKind {
    match (old, new, complete) {
        (1, 1, true) => TraceContinuationKind::Isomorphism,
        (1, count, true) if count > 1 => TraceContinuationKind::Split,
        (count, 1, true) if count > 1 => TraceContinuationKind::Merge,
        (old, new, true) if old > 1 && new > 1 => TraceContinuationKind::Mixing,
        _ => TraceContinuationKind::Ambiguous,
    }
}

pub(super) fn correspondences(
    old_graph: &ProgramGraph,
    old_spaces: &[ResultSpace],
    new_graph: &ProgramGraph,
    new_spaces: &[ResultSpace],
    modulus: u32,
    limits: &ProgramTraceProofLimits,
) -> Result<Vec<TraceCorrespondence>, ProofError> {
    if old_graph.vertex_count() != new_graph.vertex_count() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    let mut vector_total = 0;
    let mut term_total = 0;
    for old in old_spaces {
        for new in new_spaces {
            let Some(candidate) =
                relation_for_spaces(old_graph, old, new_graph, new, modulus, limits)?
            else {
                continue;
            };
            append_correspondence(
                &mut output,
                &mut vector_total,
                &mut term_total,
                old,
                new,
                candidate,
                limits,
            )?;
        }
    }
    output.sort_by(|left, right| {
        left.old_space
            .cmp(&right.old_space)
            .then(left.new_space.cmp(&right.new_space))
            .then(left.scale.total_cmp(&right.scale))
    });
    Ok(output)
}

struct CorrespondenceCandidate {
    scale: f64,
    old_image_rank: usize,
    new_image_rank: usize,
    relation: Vec<(Vec<u64>, Vec<u64>)>,
}

fn relation_for_spaces(
    old_graph: &ProgramGraph,
    old: &ResultSpace,
    new_graph: &ProgramGraph,
    new: &ResultSpace,
    modulus: u32,
    limits: &ProgramTraceProofLimits,
) -> Result<Option<CorrespondenceCandidate>, ProofError> {
    let Some(scale) = comparison_scale(old, new) else {
        return Ok(None);
    };
    let edges: Vec<_> = old_graph
        .edges()
        .iter()
        .filter(|edge| edge.value <= scale && new_graph.get(edge.u, edge.v) <= scale)
        .map(|edge| (edge.u, edge.v))
        .collect();
    let old_rows = restricted_rows(old_graph.vertex_count(), &edges, old, modulus)?;
    let new_rows = restricted_rows(new_graph.vertex_count(), &edges, new, modulus)?;
    let old_image = independent_image(&old_rows, modulus, limits.max_relation_cells)?;
    let new_image = independent_image(&new_rows, modulus, limits.max_relation_cells)?;
    let relation = image_intersection(
        &old_image,
        &new_image,
        edges.len(),
        modulus,
        limits.max_relation_cells,
    )?;
    if relation.is_empty() {
        return Ok(None);
    }
    Ok(Some(CorrespondenceCandidate {
        scale,
        old_image_rank: old_image.len(),
        new_image_rank: new_image.len(),
        relation,
    }))
}

fn append_correspondence(
    output: &mut Vec<TraceCorrespondence>,
    vector_total: &mut usize,
    term_total: &mut usize,
    old: &ResultSpace,
    new: &ResultSpace,
    candidate: CorrespondenceCandidate,
    limits: &ProgramTraceProofLimits,
) -> Result<(), ProofError> {
    if output.len() == limits.max_correspondences {
        return Err(ProofError::new(
            "trace correspondences exceed the correspondence limit",
        ));
    }
    *vector_total = bounded_add(
        *vector_total,
        candidate.relation.len(),
        limits.max_correspondence_vectors,
        "trace correspondence vectors",
    )?;
    let relation_terms = relation_term_count(&candidate.relation)?;
    *term_total = bounded_add(
        *term_total,
        relation_terms,
        limits.max_correspondence_terms,
        "trace correspondence terms",
    )?;
    let basis = candidate
        .relation
        .into_iter()
        .map(|(left, right)| (terms_for(&old.basis, &left), terms_for(&new.basis, &right)))
        .collect::<Vec<_>>();
    output.push(TraceCorrespondence {
        old_space: old.id,
        new_space: new.id,
        scale: candidate.scale,
        old_rank: old.basis.len(),
        new_rank: new.basis.len(),
        old_image_rank: candidate.old_image_rank,
        new_image_rank: candidate.new_image_rank,
        relation_rank: basis.len(),
        basis,
    });
    Ok(())
}

fn relation_term_count(relation: &[(Vec<u64>, Vec<u64>)]) -> Result<usize, ProofError> {
    relation.iter().try_fold(0usize, |total, (left, right)| {
        total
            .checked_add(left.iter().filter(|&&value| value != 0).count())
            .and_then(|total| total.checked_add(right.iter().filter(|&&value| value != 0).count()))
            .ok_or_else(|| ProofError::new("correspondence term count overflows usize"))
    })
}

fn bounded_add(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new(format!("{label} count overflows usize")))?;
    if next > limit {
        return Err(ProofError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

fn push_bounded<T>(
    output: &mut Vec<T>,
    value: T,
    limit: usize,
    label: &str,
) -> Result<(), ProofError> {
    if output.len() == limit {
        return Err(ProofError::new(format!("{label} exceed the limit {limit}")));
    }
    output.push(value);
    Ok(())
}

fn comparison_scale(old: &ResultSpace, new: &ResultSpace) -> Option<f64> {
    let birth = old.interval.birth.max(new.interval.birth);
    let death = old.interval.death.min(new.interval.death);
    let scale = if death.is_infinite() {
        old.scale.max(new.scale).max(birth)
    } else {
        old.scale.min(new.scale)
    };
    (scale >= birth && scale < death).then_some(scale)
}
