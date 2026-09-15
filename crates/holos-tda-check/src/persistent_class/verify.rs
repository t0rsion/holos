use std::collections::BTreeMap;

use crate::program::{ReplayGroup, ReplayPair, replay_h1};
use crate::proof::{Graph, ProofBar, ProofEdge};
use crate::{ProgramProofLimits, ProofError, ProofLimits};

use super::model::{
    DecodedPersistentClass, PersistenceCycleTerm, PersistentCocycleTerm, PersistentCriticalPair,
    VerifiedPersistentClass,
};
use super::wire::{MAGIC, decode};

/// Return true when bytes start with a `HOLOSPC` persistent-class envelope.
pub fn is_persistent_class(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one bounded `HOLOSPC` persistent H1 class artifact.
pub fn verify_persistent_class(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedPersistentClass, ProofError> {
    verify_decoded(decode(bytes, limits)?, limits)
}

fn verify_decoded(
    claim: DecodedPersistentClass,
    limits: ProofLimits,
) -> Result<VerifiedPersistentClass, ProofError> {
    let graph = source_graph(&claim)?;
    let groups = replay_groups(&graph, &claim, limits)?;
    let group = checked_group(&groups, &claim)?;
    checked_pair(group, &graph, &claim)?;
    check_witnesses(&graph, &claim)?;
    Ok(claim.into_verified())
}

fn replay_groups(
    graph: &Graph,
    claim: &DecodedPersistentClass,
    limits: ProofLimits,
) -> Result<Vec<ReplayGroup>, ProofError> {
    Ok(replay_h1(graph, claim.threshold, claim.modulus, replay_limits(limits))?.groups)
}

fn checked_group<'a>(
    groups: &'a [ReplayGroup],
    claim: &DecodedPersistentClass,
) -> Result<&'a ReplayGroup, ProofError> {
    let group = find_group(groups, claim.interval, claim.group_id)?;
    check_group_header(group, claim)?;
    check_basis_index(group, claim.basis_index)?;
    check_selected_class(group, claim)?;
    Ok(group)
}

fn check_basis_index(group: &ReplayGroup, basis_index: usize) -> Result<(), ProofError> {
    if basis_index >= group.basis.len() {
        return Err(ProofError::new(
            "persistent-class basis index is outside its group",
        ));
    }
    Ok(())
}

fn checked_pair<'a>(
    group: &'a ReplayGroup,
    graph: &Graph,
    claim: &DecodedPersistentClass,
) -> Result<&'a ReplayPair, ProofError> {
    let pair = find_pair(group, claim.pair)?;
    check_selected_pair(graph, claim, pair)?;
    Ok(pair)
}

fn check_witnesses(graph: &Graph, claim: &DecodedPersistentClass) -> Result<(), ProofError> {
    check_cocycle_support(graph, claim)?;
    let cycle = check_cycle(graph, claim)?;
    check_pairing(&claim.cocycle, &cycle, claim.modulus)?;
    check_bounding_chain(graph, claim, &cycle)?;
    Ok(())
}

fn replay_limits(limits: ProofLimits) -> ProgramProofLimits {
    ProgramProofLimits {
        max_bytes: limits.max_bytes,
        max_vertices: limits.max_vertices,
        max_atoms: limits.max_bars,
        max_atom_vertices: limits.max_vertices,
        max_atom_edges: limits.max_edges,
        max_bars: limits.max_bars,
        max_atlas_bytes: limits.max_bytes,
        max_nested_atlas_bytes: limits.max_bytes,
        max_spaces: limits.max_bars,
        max_basis: limits.max_bars,
        max_critical_pairs: limits.max_bars,
        max_cocycle_terms: limits.max_terms,
        max_certificate_bytes: limits.max_bytes,
        max_edges: limits.max_edges,
        max_triangles: limits.max_triangles,
        max_terms: limits.max_terms,
    }
}

fn source_graph(claim: &DecodedPersistentClass) -> Result<Graph, ProofError> {
    let edges = claim
        .source
        .iter()
        .map(|edge| ProofEdge {
            u: edge.u,
            v: edge.v,
            value: edge.value,
        })
        .collect::<Vec<_>>();
    Graph::new(claim.vertex_count, &edges)
}

fn find_group(
    groups: &[ReplayGroup],
    interval: ProofBar,
    id: [u8; 32],
) -> Result<&ReplayGroup, ProofError> {
    let mut matches = groups
        .iter()
        .filter(|group| same_bar(group.interval, interval));
    let group = matches
        .next()
        .ok_or_else(|| ProofError::new("persistent-class interval is absent from replay"))?;
    if matches.next().is_some() {
        return Err(ProofError::new(
            "persistent-class replay returned duplicate interval groups",
        ));
    }
    if group.id != id {
        return Err(ProofError::new(
            "persistent-class group identifier differs from replay",
        ));
    }
    Ok(group)
}

fn check_group_header(
    group: &ReplayGroup,
    claim: &DecodedPersistentClass,
) -> Result<(), ProofError> {
    if !same_bar(group.interval, claim.interval)
        || group.scale.to_bits() != claim.scale.to_bits()
        || group.basis.is_empty()
        || group.basis.len() != group.pairs.len()
        || group.basis.len() != group.class_ids.len()
    {
        return Err(ProofError::new(
            "persistent-class replay group header differs from the artifact",
        ));
    }
    Ok(())
}

fn check_selected_class(
    group: &ReplayGroup,
    claim: &DecodedPersistentClass,
) -> Result<(), ProofError> {
    let basis = &group.basis[claim.basis_index];
    if group.class_ids.get(claim.basis_index).copied() != Some(claim.class_id)
        || !same_bar(group.interval, claim.interval)
        || group.scale.to_bits() != claim.scale.to_bits()
        || basis.len() != claim.cocycle.len()
        || basis.iter().zip(&claim.cocycle).any(|(expected, actual)| {
            expected.u != actual.u
                || expected.v != actual.v
                || expected.coefficient != actual.coefficient
        })
    {
        return Err(ProofError::new(
            "persistent-class cocycle differs from the complete replay",
        ));
    }
    Ok(())
}

fn find_pair(group: &ReplayGroup, pair: PersistentCriticalPair) -> Result<&ReplayPair, ProofError> {
    let mut matches = group.pairs.iter().filter(|candidate| {
        same_bar(candidate.interval, group.interval)
            && candidate.birth == pair.birth
            && candidate.death == pair.death
    });
    let selected = matches.next().ok_or_else(|| {
        ProofError::new("persistent-class critical pair is absent from its group")
    })?;
    if matches.next().is_some() {
        return Err(ProofError::new(
            "persistent-class group contains duplicate critical pairs",
        ));
    }
    Ok(selected)
}

fn check_selected_pair(
    graph: &Graph,
    claim: &DecodedPersistentClass,
    pair: &ReplayPair,
) -> Result<(), ProofError> {
    check_pair_identity(claim, pair)?;
    check_birth_filtration(graph, claim, pair)?;
    match pair.death {
        None => check_essential_pair(claim),
        Some(death) => check_finite_pair(graph, claim, death),
    }
}

fn check_pair_identity(
    claim: &DecodedPersistentClass,
    pair: &ReplayPair,
) -> Result<(), ProofError> {
    if claim.pair.birth != pair.birth || claim.pair.death != pair.death {
        return Err(ProofError::new(
            "persistent-class critical pair differs from replay",
        ));
    }
    Ok(())
}

fn check_birth_filtration(
    graph: &Graph,
    claim: &DecodedPersistentClass,
    pair: &ReplayPair,
) -> Result<(), ProofError> {
    let birth = graph.get(pair.birth[0], pair.birth[1]);
    if birth.to_bits() != claim.interval.birth.to_bits() {
        return Err(ProofError::new(
            "persistent-class birth edge has the wrong filtration value",
        ));
    }
    Ok(())
}

fn check_essential_pair(claim: &DecodedPersistentClass) -> Result<(), ProofError> {
    if !claim.interval.death.is_infinite() {
        return Err(ProofError::new(
            "persistent-class critical pair has the wrong death presence",
        ));
    }
    Ok(())
}

fn check_finite_pair(
    graph: &Graph,
    claim: &DecodedPersistentClass,
    death: [usize; 3],
) -> Result<(), ProofError> {
    if !claim.interval.death.is_finite() {
        return Err(ProofError::new(
            "persistent-class critical pair has the wrong death presence",
        ));
    }
    let value = triangle_value(graph, death).ok_or_else(|| {
        ProofError::new("persistent-class death triangle contains an absent edge")
    })?;
    if value.to_bits() != claim.interval.death.to_bits() {
        return Err(ProofError::new(
            "persistent-class death triangle has the wrong filtration value",
        ));
    }
    Ok(())
}

fn check_cocycle_support(graph: &Graph, claim: &DecodedPersistentClass) -> Result<(), ProofError> {
    if claim.cocycle.is_empty() {
        return Err(ProofError::new("persistent-class cocycle has no terms"));
    }
    if claim.cocycle[0].coefficient != 1 {
        return Err(ProofError::new(
            "persistent-class cocycle is not normalized",
        ));
    }
    for term in &claim.cocycle {
        if !graph.get(term.u, term.v).is_finite() || graph.get(term.u, term.v) > claim.scale {
            return Err(ProofError::new(
                "persistent-class cocycle uses an inactive edge",
            ));
        }
    }
    Ok(())
}

fn check_cycle(
    graph: &Graph,
    claim: &DecodedPersistentClass,
) -> Result<BTreeMap<(usize, usize), u64>, ProofError> {
    if claim.cycle.is_empty() {
        return Err(ProofError::new("persistent-class birth cycle has no terms"));
    }
    if !claim
        .cycle
        .iter()
        .any(|term| term.u == claim.pair.birth[0] && term.v == claim.pair.birth[1])
    {
        return Err(ProofError::new(
            "persistent-class birth cycle omits its critical birth edge",
        ));
    }
    let coefficients = cycle_coefficients(&claim.cycle);
    let mut boundary = vec![0u64; graph.vertex_count];
    for (&(u, v), &coefficient) in &coefficients {
        let value = graph.get(u, v);
        if !value.is_finite() || value > claim.interval.birth {
            return Err(ProofError::new(
                "persistent-class birth cycle uses an edge born after its pair",
            ));
        }
        boundary[u] = sub_mod(boundary[u], coefficient, claim.modulus as u64);
        boundary[v] = add_mod(boundary[v], coefficient, claim.modulus as u64);
    }
    if boundary.into_iter().any(|value| value != 0) {
        return Err(ProofError::new(
            "persistent-class birth cycle is not closed",
        ));
    }
    Ok(coefficients)
}

fn check_pairing(
    cocycle: &[PersistentCocycleTerm],
    cycle: &BTreeMap<(usize, usize), u64>,
    modulus: u32,
) -> Result<(), ProofError> {
    let coefficients = cocycle
        .iter()
        .map(|term| ((term.u, term.v), u64::from(term.coefficient)))
        .collect::<BTreeMap<_, _>>();
    let pairing = cycle.iter().fold(0u64, |sum, (&edge, &coefficient)| {
        (sum + coefficients.get(&edge).copied().unwrap_or(0) * coefficient) % u64::from(modulus)
    });
    if pairing != 1 {
        return Err(ProofError::new(
            "persistent-class cocycle and birth cycle do not pair to one",
        ));
    }
    Ok(())
}

fn check_bounding_chain(
    graph: &Graph,
    claim: &DecodedPersistentClass,
    cycle: &BTreeMap<(usize, usize), u64>,
) -> Result<(), ProofError> {
    let Some(death) = claim.pair.death else {
        if !claim.bounding_chain.is_empty() {
            return Err(ProofError::new(
                "persistent-class essential witness has a bounding chain",
            ));
        }
        return Ok(());
    };
    if claim.bounding_chain.is_empty() {
        return Err(ProofError::new(
            "persistent-class finite witness has no bounding chain",
        ));
    }
    if !claim
        .bounding_chain
        .iter()
        .any(|term| term.vertices == death)
    {
        return Err(ProofError::new(
            "persistent-class bounding chain omits its critical death triangle",
        ));
    }
    let modulus = u64::from(claim.modulus);
    let mut boundary = BTreeMap::new();
    for term in &claim.bounding_chain {
        let value = triangle_value(graph, term.vertices).ok_or_else(|| {
            ProofError::new("persistent-class bounding chain contains an absent triangle")
        })?;
        if value > claim.interval.death {
            return Err(ProofError::new(
                "persistent-class bounding chain uses a triangle born after death",
            ));
        }
        let [u, v, w] = term.vertices;
        add_edge_coefficient(&mut boundary, (v, w), u64::from(term.coefficient), modulus);
        add_edge_coefficient(
            &mut boundary,
            (u, w),
            modulus - u64::from(term.coefficient),
            modulus,
        );
        add_edge_coefficient(&mut boundary, (u, v), u64::from(term.coefficient), modulus);
    }
    if boundary != *cycle {
        return Err(ProofError::new(
            "persistent-class bounding-chain boundary differs from its cycle",
        ));
    }
    Ok(())
}

fn triangle_value(graph: &Graph, [u, v, w]: [usize; 3]) -> Option<f64> {
    let uv = graph.get(u, v);
    let uw = graph.get(u, w);
    let vw = graph.get(v, w);
    (uv.is_finite() && uw.is_finite() && vw.is_finite()).then(|| uv.max(uw).max(vw))
}

fn cycle_coefficients(terms: &[PersistenceCycleTerm]) -> BTreeMap<(usize, usize), u64> {
    terms
        .iter()
        .map(|term| ((term.u, term.v), u64::from(term.coefficient)))
        .collect()
}

fn add_edge_coefficient(
    coefficients: &mut BTreeMap<(usize, usize), u64>,
    edge: (usize, usize),
    coefficient: u64,
    modulus: u64,
) {
    let next = (coefficients.get(&edge).copied().unwrap_or(0) + coefficient) % modulus;
    if next == 0 {
        coefficients.remove(&edge);
    } else {
        coefficients.insert(edge, next);
    }
}

fn add_mod(left: u64, right: u64, modulus: u64) -> u64 {
    (left + right) % modulus
}

fn sub_mod(left: u64, right: u64, modulus: u64) -> u64 {
    (left + modulus - right) % modulus
}

fn same_bar(left: ProofBar, right: ProofBar) -> bool {
    left.dimension == right.dimension
        && left.birth.to_bits() == right.birth.to_bits()
        && left.death.to_bits() == right.death.to_bits()
}
