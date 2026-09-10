use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{BasisClassId, CocycleTerm, IntervalGroupId, PersistentClassSpace};

use super::model::{BasisTransport, ClassContinuation, ContinuationKind};

type BasisTerms = BTreeMap<Vec<CocycleTerm>, Vec<(usize, BasisClassId)>>;
type ContinuationTransports = BTreeMap<(usize, usize), Vec<BasisTransport>>;

pub(crate) fn class_continuation(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
) -> Vec<ClassContinuation> {
    let old_terms = basis_terms(old);
    let new_terms = basis_terms(new);
    let (adjacency, transports) = continuation_graph(old.len(), new.len(), &old_terms, &new_terms);
    let mut seen = vec![false; adjacency.len()];
    let mut output = Vec::new();
    for start in 0..adjacency.len() {
        if seen[start] || adjacency[start].is_empty() {
            continue;
        }
        let (old_nodes, new_nodes) =
            continuation_component(start, old.len(), &adjacency, &mut seen);
        output.push(component_continuation(
            old,
            new,
            &old_nodes,
            &new_nodes,
            &transports,
        ));
    }
    output.extend(unmatched_continuations(old, new, &adjacency));
    output.sort_by(|a, b| {
        a.old_spaces
            .cmp(&b.old_spaces)
            .then(a.new_spaces.cmp(&b.new_spaces))
    });
    output
}

fn basis_terms(spaces: &[PersistentClassSpace]) -> BasisTerms {
    let mut terms = BasisTerms::new();
    for (space, item) in spaces.iter().enumerate() {
        for class in &item.basis {
            terms
                .entry(class.cocycle.terms.clone())
                .or_default()
                .push((space, class.id));
        }
    }
    terms
}

fn continuation_graph(
    old_count: usize,
    new_count: usize,
    old_terms: &BasisTerms,
    new_terms: &BasisTerms,
) -> (Vec<BTreeSet<usize>>, ContinuationTransports) {
    let mut adjacency = vec![BTreeSet::new(); old_count + new_count];
    let mut transports = ContinuationTransports::new();
    for (terms, old_basis) in old_terms {
        let Some(new_basis) = new_terms.get(terms) else {
            continue;
        };
        connect_matching_basis(
            old_count,
            old_basis,
            new_basis,
            &mut adjacency,
            &mut transports,
        );
    }
    (adjacency, transports)
}

fn connect_matching_basis(
    old_count: usize,
    old_basis: &[(usize, BasisClassId)],
    new_basis: &[(usize, BasisClassId)],
    adjacency: &mut [BTreeSet<usize>],
    transports: &mut ContinuationTransports,
) {
    for &(old_space, old_id) in old_basis {
        for &(new_space, new_id) in new_basis {
            let new_node = old_count + new_space;
            adjacency[old_space].insert(new_node);
            adjacency[new_node].insert(old_space);
            transports
                .entry((old_space, new_space))
                .or_default()
                .push(BasisTransport {
                    old: old_id,
                    new: new_id,
                    coefficient: 1,
                });
        }
    }
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
        enqueue_unseen(&adjacency[node], seen, &mut queue);
    }
    old_nodes.sort_unstable();
    new_nodes.sort_unstable();
    (old_nodes, new_nodes)
}

fn enqueue_unseen(adjacency: &BTreeSet<usize>, seen: &mut [bool], queue: &mut VecDeque<usize>) {
    for &next in adjacency {
        if !seen[next] {
            seen[next] = true;
            queue.push_back(next);
        }
    }
}

fn component_continuation(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
    old_nodes: &[usize],
    new_nodes: &[usize],
    transports: &ContinuationTransports,
) -> ClassContinuation {
    let mut transport = component_transports(old_nodes, new_nodes, transports);
    transport.sort_by_key(|term| (term.old, term.new));
    transport.dedup();
    let old_rank: usize = old_nodes.iter().map(|&index| old[index].basis.len()).sum();
    let new_rank: usize = new_nodes.iter().map(|&index| new[index].basis.len()).sum();
    let complete = transport.len() == old_rank && transport.len() == new_rank;
    ClassContinuation {
        kind: continuation_kind(old_nodes.len(), new_nodes.len(), complete),
        old_spaces: old_nodes.iter().map(|&index| old[index].id).collect(),
        new_spaces: new_nodes.iter().map(|&index| new[index].id).collect(),
        transport,
    }
}

fn component_transports(
    old_nodes: &[usize],
    new_nodes: &[usize],
    transports: &ContinuationTransports,
) -> Vec<BasisTransport> {
    let mut output = Vec::new();
    for &old_space in old_nodes {
        for &new_space in new_nodes {
            if let Some(terms) = transports.get(&(old_space, new_space)) {
                output.extend(terms.iter().copied());
            }
        }
    }
    output
}

fn continuation_kind(old_count: usize, new_count: usize, complete: bool) -> ContinuationKind {
    match (old_count, new_count, complete) {
        (1, 1, true) => ContinuationKind::Isomorphism,
        (1, many, true) if many > 1 => ContinuationKind::Split,
        (many, 1, true) if many > 1 => ContinuationKind::Merge,
        (many_old, many_new, true) if many_old > 1 && many_new > 1 => ContinuationKind::Mixing,
        _ => ContinuationKind::Ambiguous,
    }
}

fn unmatched_continuations(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
    adjacency: &[BTreeSet<usize>],
) -> Vec<ClassContinuation> {
    let deaths = old
        .iter()
        .enumerate()
        .filter(|(index, _)| adjacency[*index].is_empty())
        .map(|(_, space)| unmatched_continuation(ContinuationKind::Death, space.id));
    let births = new
        .iter()
        .enumerate()
        .filter(|(index, _)| adjacency[old.len() + *index].is_empty())
        .map(|(_, space)| unmatched_continuation(ContinuationKind::Birth, space.id));
    deaths.chain(births).collect()
}

fn unmatched_continuation(kind: ContinuationKind, space: IntervalGroupId) -> ClassContinuation {
    let (old_spaces, new_spaces) = match kind {
        ContinuationKind::Death => (vec![space], Vec::new()),
        ContinuationKind::Birth => (Vec::new(), vec![space]),
        _ => unreachable!("only births and deaths are unmatched"),
    };
    ClassContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport: Vec::new(),
    }
}
