use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::program::{ReplayCocycleTerm, ReplayGroup, ReplayPair, ReplayResult, replay_h1};
use crate::proof::{Graph, ProofEdge};
use crate::{ProgramProofLimits, ProofLimits, is_persistent_class, verify_persistent_class};

fn square_artifact() -> Vec<u8> {
    let source = [
        (0usize, 1usize, 1.0f64),
        (0, 3, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
    ];
    replayed_artifact(&source, 5, None, &[])
}

fn finite_square_artifact() -> Vec<u8> {
    let source = [
        (0usize, 1usize, 1.0f64),
        (0, 2, 2.0),
        (0, 3, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
    ];
    replayed_artifact(&source, 4, None, &[(0, 1, 2, 1), (0, 2, 3, 1)])
}

fn replayed_artifact(
    source: &[(usize, usize, f64)],
    vertex_count: usize,
    threshold: Option<f64>,
    chain: &[(usize, usize, usize, u32)],
) -> Vec<u8> {
    let graph = Graph::new(
        vertex_count,
        &source
            .iter()
            .map(|&(u, v, value)| ProofEdge { u, v, value })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let replay = replay_h1(&graph, threshold, 5, ProgramProofLimits::default()).unwrap();
    let group = &replay.groups[0];
    let cycle = normalized_square_cycle(group, 5);

    encode_artifact_claim(ArtifactClaim {
        source,
        vertex_count,
        threshold,
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id: group.id,
        class_id: group.class_ids[0],
        basis_index: 0,
        basis: &group.basis[0],
        pair: &group.pairs[0],
        cycle: &cycle,
        chain,
    })
}

struct ArtifactClaim<'a> {
    source: &'a [(usize, usize, f64)],
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    interval: crate::proof::ProofBar,
    scale: f64,
    group_id: [u8; 32],
    class_id: [u8; 32],
    basis_index: usize,
    basis: &'a [ReplayCocycleTerm],
    pair: &'a ReplayPair,
    cycle: &'a [(usize, usize, u32)],
    chain: &'a [(usize, usize, usize, u32)],
}

fn encode_artifact_claim(claim: ArtifactClaim<'_>) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"HOLOSPC\0");
    put_u16(&mut payload, 1);
    payload.push(1);
    put_u32(&mut payload, claim.modulus);
    put_u64(&mut payload, claim.vertex_count as u64);
    put_u64(&mut payload, claim.source.len() as u64);
    for &(u, v, value) in claim.source {
        put_u64(&mut payload, u as u64);
        put_u64(&mut payload, v as u64);
        put_u64(&mut payload, value.to_bits());
    }
    match claim.threshold {
        None => payload.push(0),
        Some(value) => {
            payload.push(1);
            put_u64(&mut payload, value.to_bits());
        }
    }
    payload.extend_from_slice(&claim.group_id);
    payload.extend_from_slice(&claim.class_id);
    put_u64(&mut payload, claim.basis_index as u64);
    put_u64(&mut payload, claim.interval.birth.to_bits());
    put_u64(&mut payload, claim.interval.death.to_bits());
    put_u64(&mut payload, claim.scale.to_bits());
    put_u64(&mut payload, claim.basis.len() as u64);
    for term in claim.basis {
        put_u64(&mut payload, term.u as u64);
        put_u64(&mut payload, term.v as u64);
        put_u32(&mut payload, term.coefficient);
    }
    put_u64(&mut payload, claim.pair.birth[0] as u64);
    put_u64(&mut payload, claim.pair.birth[1] as u64);
    match claim.pair.death {
        None => payload.push(0),
        Some([u, v, w]) => {
            payload.push(1);
            put_u64(&mut payload, u as u64);
            put_u64(&mut payload, v as u64);
            put_u64(&mut payload, w as u64);
        }
    }
    put_u64(&mut payload, claim.cycle.len() as u64);
    for &(u, v, coefficient) in claim.cycle {
        put_u64(&mut payload, u as u64);
        put_u64(&mut payload, v as u64);
        put_u32(&mut payload, coefficient);
    }
    put_u64(&mut payload, claim.chain.len() as u64);
    for &(u, v, w, coefficient) in claim.chain {
        put_u64(&mut payload, u as u64);
        put_u64(&mut payload, v as u64);
        put_u64(&mut payload, w as u64);
        put_u32(&mut payload, coefficient);
    }
    seal(payload)
}

fn normalized_square_cycle(group: &ReplayGroup, modulus: u64) -> Vec<(usize, usize, u32)> {
    normalized_square_cycle_for_basis(group, 0, 0, modulus)
}

fn normalized_square_cycle_for_basis(
    group: &ReplayGroup,
    basis_index: usize,
    offset: usize,
    modulus: u64,
) -> Vec<(usize, usize, u32)> {
    let raw = [
        (offset, offset + 1, 1u64),
        (offset, offset + 3, modulus - 1),
        (offset + 1, offset + 2, 1),
        (offset + 2, offset + 3, 1),
    ];
    let pairing = raw.iter().fold(0, |sum, &(u, v, coefficient)| {
        let cocycle_coefficient = group.basis[basis_index]
            .iter()
            .find(|term| term.u == u && term.v == v)
            .map_or(0, |term| u64::from(term.coefficient));
        (sum + coefficient * cocycle_coefficient) % modulus
    });
    assert_ne!(pairing, 0);
    let inverse = crate::inverse_mod(pairing, modulus);
    raw.into_iter()
        .map(|(u, v, coefficient)| (u, v, (coefficient * inverse % modulus) as u32))
        .collect()
}

fn overlapping_square_source() -> Vec<(usize, usize, f64)> {
    let mut source = Vec::new();
    append_square(&mut source, 0, 1.0, 3.0);
    source.push((3, 4, 1.5));
    append_square(&mut source, 4, 2.0, 4.0);
    source.push((7, 8, 2.5));
    append_square(&mut source, 8, 1.0, 3.0);
    source
}

fn append_square(source: &mut Vec<(usize, usize, f64)>, offset: usize, edge: f64, diagonal: f64) {
    source.extend([
        (offset, offset + 1, edge),
        (offset, offset + 2, diagonal),
        (offset, offset + 3, edge),
        (offset + 1, offset + 2, edge),
        (offset + 2, offset + 3, edge),
    ]);
}

fn overlapping_replay(threshold: Option<f64>) -> (Vec<(usize, usize, f64)>, ReplayResult) {
    let source = overlapping_square_source();
    let graph = Graph::new(
        12,
        &source
            .iter()
            .map(|&(u, v, value)| ProofEdge { u, v, value })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let replay = replay_h1(&graph, threshold, 5, ProgramProofLimits::default()).unwrap();
    (source, replay)
}

fn group_with_interval(replay: &ReplayResult, birth: f64, death: f64) -> &ReplayGroup {
    replay
        .groups
        .iter()
        .find(|group| {
            group.interval.birth.to_bits() == birth.to_bits()
                && group.interval.death.to_bits() == death.to_bits()
        })
        .expect("test source has the requested persistence group")
}

fn component_basis_index(group: &ReplayGroup, offset: usize) -> usize {
    group
        .basis
        .iter()
        .position(|terms| {
            terms
                .iter()
                .any(|term| offset <= term.u && term.v <= offset + 3)
        })
        .expect("test group has a basis class on the requested component")
}

fn component_pair(group: &ReplayGroup, offset: usize) -> &ReplayPair {
    group
        .pairs
        .iter()
        .find(|pair| offset <= pair.birth[0] && pair.birth[1] <= offset + 3)
        .expect("test group has a critical pair on the requested component")
}

fn square_chain(offset: usize) -> [(usize, usize, usize, u32); 2] {
    square_chain_scaled(offset, 1)
}

fn square_chain_scaled(offset: usize, coefficient: u32) -> [(usize, usize, usize, u32); 2] {
    [
        (offset, offset + 1, offset + 2, coefficient),
        (offset, offset + 2, offset + 3, coefficient),
    ]
}

fn cycle_edge_coefficient(cycle: &[(usize, usize, u32)], edge: (usize, usize)) -> u32 {
    cycle
        .iter()
        .find(|&&(u, v, _)| (u, v) == edge)
        .map_or(0, |&(_, _, coefficient)| coefficient)
}

fn component_artifact(
    threshold: Option<f64>,
    interval_birth: f64,
    interval_death: f64,
    offset: usize,
) -> Vec<u8> {
    let (source, replay) = overlapping_replay(threshold);
    let group = group_with_interval(&replay, interval_birth, interval_death);
    let basis_index = component_basis_index(group, offset);
    let pair = component_pair(group, offset);
    let cycle = normalized_square_cycle_for_basis(group, basis_index, offset, 5);
    let chain = if pair.death.is_some() {
        square_chain_scaled(offset, cycle_edge_coefficient(&cycle, (offset, offset + 1))).to_vec()
    } else {
        Vec::new()
    };
    encode_artifact_claim(ArtifactClaim {
        source: &source,
        vertex_count: 12,
        threshold,
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id: group.id,
        class_id: group.class_ids[basis_index],
        basis_index,
        basis: &group.basis[basis_index],
        pair,
        cycle: &cycle,
        chain: &chain,
    })
}

fn add_basis_terms(
    left: &[ReplayCocycleTerm],
    right: &[ReplayCocycleTerm],
    modulus: u32,
) -> Vec<ReplayCocycleTerm> {
    let mut coefficients = BTreeMap::new();
    for term in left.iter().chain(right) {
        let value = coefficients.entry((term.u, term.v)).or_insert(0u64);
        *value = (*value + u64::from(term.coefficient)) % u64::from(modulus);
    }
    coefficients
        .into_iter()
        .filter_map(|((u, v), coefficient)| {
            (coefficient != 0).then_some(ReplayCocycleTerm {
                u,
                v,
                coefficient: coefficient as u32,
            })
        })
        .collect()
}

fn add_cycle_terms(
    left: &[(usize, usize, u32)],
    right: &[(usize, usize, u32)],
    modulus: u32,
) -> Vec<(usize, usize, u32)> {
    let mut coefficients = BTreeMap::new();
    for &(u, v, coefficient) in left.iter().chain(right) {
        let value = coefficients.entry((u, v)).or_insert(0u64);
        *value = (*value + u64::from(coefficient)) % u64::from(modulus);
    }
    coefficients
        .into_iter()
        .filter_map(|((u, v), coefficient)| {
            (coefficient != 0).then_some((u, v, coefficient as u32))
        })
        .collect()
}

fn independent_group_id(
    interval: crate::proof::ProofBar,
    modulus: u32,
    scale: f64,
    basis: &[Vec<ReplayCocycleTerm>],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-class-space-v1");
    hash.update((interval.dimension as u64).to_be_bytes());
    hash.update(interval.birth.to_bits().to_be_bytes());
    hash.update(interval.death.to_bits().to_be_bytes());
    hash.update(modulus.to_be_bytes());
    hash.update((basis.len() as u64).to_be_bytes());
    for terms in basis {
        hash.update(scale.to_bits().to_be_bytes());
        hash.update((terms.len() as u64).to_be_bytes());
        for term in terms {
            hash.update((term.u as u64).to_be_bytes());
            hash.update((term.v as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
    hash.finalize().into()
}

fn independent_class_id(
    group_id: [u8; 32],
    basis_index: usize,
    scale: f64,
    terms: &[ReplayCocycleTerm],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-basis-class-v1");
    hash.update(group_id);
    hash.update((basis_index as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for term in terms {
        hash.update((term.u as u64).to_be_bytes());
        hash.update((term.v as u64).to_be_bytes());
        hash.update(term.coefficient.to_be_bytes());
    }
    hash.finalize().into()
}

fn seal(mut payload: Vec<u8>) -> Vec<u8> {
    let digest: [u8; 32] = Sha256::digest(&payload).into();
    payload.extend_from_slice(&digest);
    payload
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn reseal(bytes: &mut [u8]) {
    let payload_len = bytes.len() - 32;
    let digest: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    bytes[payload_len..].copy_from_slice(&digest);
}

fn assert_current_digest(bytes: &[u8]) {
    let payload_len = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    assert_eq!(&bytes[payload_len..], expected.as_slice());
}

#[test]
fn essential_square_artifact_verifies_against_complete_replay() {
    let bytes = square_artifact();
    assert!(is_persistent_class(&bytes));
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.vertex_count(), 5);
    assert_eq!(checked.source().len(), 4);
    assert_eq!(checked.interval().death, f64::INFINITY);
    assert!(checked.bounding_chain().is_empty());
}

#[test]
fn finite_square_artifact_verifies_its_death_boundary() {
    let bytes = finite_square_artifact();
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert!(checked.interval().death.is_finite());
    assert_eq!(checked.bounding_chain().len(), 2);
}

#[test]
fn resealed_cocycle_mutation_is_rejected_by_exact_class_comparison() {
    let mut bytes = square_artifact();
    let cocycle_u = 8 + 2 + 1 + 4 + 8 + 8 + 4 * 24 + 1 + 32 + 32 + 8 + 8 + 8 + 8 + 8;
    put_u64_at(&mut bytes, cocycle_u, 0);
    reseal(&mut bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn resealed_cycle_mutation_is_rejected_by_boundary_check() {
    let mut bytes = square_artifact();
    let cycle_coefficient =
        8 + 2 + 1 + 4 + 8 + 8 + 4 * 24 + 1 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 20 + 16 + 1 + 8 + 16;
    bytes[cycle_coefficient..cycle_coefficient + 4].copy_from_slice(&2u32.to_be_bytes());
    reseal(&mut bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn resealed_active_source_mutation_is_rejected_by_replay() {
    let mut bytes = square_artifact();
    let source_weight = 8 + 2 + 1 + 4 + 8 + 8 + 16;
    put_u64_at(&mut bytes, source_weight, 0.5f64.to_bits());
    reseal(&mut bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

fn put_u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}

fn put_u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

#[test]
fn resealed_repeated_interval_mixture_is_rejected_by_complete_replay() {
    let (source, replay) = overlapping_replay(None);
    let group = group_with_interval(&replay, 1.0, 3.0);
    assert_eq!(group.basis.len(), 2);
    let a_index = component_basis_index(group, 0);
    let c_index = component_basis_index(group, 8);
    assert_ne!(a_index, c_index);
    let valid = component_artifact(None, 1.0, 3.0, 0);
    assert!(verify_persistent_class(&valid, ProofLimits::default()).is_ok());

    let mixed = add_basis_terms(&group.basis[a_index], &group.basis[c_index], 5);
    let mut basis = group.basis.clone();
    basis[a_index] = mixed.clone();
    let group_id = independent_group_id(group.interval, 5, group.scale, &basis);
    let class_id = independent_class_id(group_id, a_index, group.scale, &mixed);
    let pair = component_pair(group, 0);
    let cycle = normalized_square_cycle_for_basis(group, a_index, 0, 5);
    let bytes = encode_artifact_claim(ArtifactClaim {
        source: &source,
        vertex_count: 12,
        threshold: None,
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id,
        class_id,
        basis_index: a_index,
        basis: &mixed,
        pair,
        cycle: &cycle,
        chain: &[],
    });
    assert_current_digest(&bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn resealed_other_interval_cocycle_is_rejected_with_recomputed_ids() {
    let (source, replay) = overlapping_replay(None);
    let group = group_with_interval(&replay, 1.0, 3.0);
    let a_index = component_basis_index(group, 0);
    let other_group = group_with_interval(&replay, 2.0, 4.0);
    let other_index = component_basis_index(other_group, 4);
    let valid = component_artifact(None, 2.0, 4.0, 4);
    assert!(verify_persistent_class(&valid, ProofLimits::default()).is_ok());
    let other_cocycle = other_group.basis[other_index].clone();
    let mut basis = group.basis.clone();
    basis[a_index] = other_cocycle.clone();
    let group_id = independent_group_id(group.interval, 5, group.scale, &basis);
    let class_id = independent_class_id(group_id, a_index, group.scale, &other_cocycle);
    let pair = component_pair(group, 0);
    let cycle = normalized_square_cycle_for_basis(group, a_index, 0, 5);
    let bytes = encode_artifact_claim(ArtifactClaim {
        source: &source,
        vertex_count: 12,
        threshold: None,
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id,
        class_id,
        basis_index: a_index,
        basis: &other_cocycle,
        pair,
        cycle: &cycle,
        chain: &[],
    });
    assert_current_digest(&bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn repeated_interval_pair_is_checked_at_group_level() {
    let (source, replay) = overlapping_replay(None);
    let group = group_with_interval(&replay, 1.0, 3.0);
    let a_index = component_basis_index(group, 0);
    let a_cycle = normalized_square_cycle_for_basis(group, a_index, 0, 5);
    let c_index = component_basis_index(group, 8);
    let c_pair = component_pair(group, 8);
    let c_cycle = normalized_square_cycle_for_basis(group, c_index, 8, 5);
    let cycle = add_cycle_terms(&a_cycle, &c_cycle, 5);
    let a_chain = square_chain_scaled(0, cycle_edge_coefficient(&a_cycle, (0, 1)));
    let c_chain = square_chain_scaled(8, cycle_edge_coefficient(&c_cycle, (8, 9)));
    let chain: Vec<_> = a_chain.into_iter().chain(c_chain).collect();
    let bytes = encode_artifact_claim(ArtifactClaim {
        source: &source,
        vertex_count: 12,
        threshold: None,
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id: group.id,
        class_id: group.class_ids[a_index],
        basis_index: a_index,
        basis: &group.basis[a_index],
        pair: c_pair,
        cycle: &cycle,
        chain: &chain,
    });
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.critical_pair().birth(), c_pair.birth);
}

#[test]
fn resealed_false_pair_birth_and_death_are_rejected_with_consistent_ids() {
    let (source, replay) = overlapping_replay(None);
    let group = group_with_interval(&replay, 1.0, 3.0);
    let basis_index = component_basis_index(group, 0);
    let basis = &group.basis[basis_index];
    let a_pair = component_pair(group, 0);
    let cycle = normalized_square_cycle_for_basis(group, basis_index, 0, 5);
    let other_group = group_with_interval(&replay, 2.0, 4.0);
    let false_birth = component_pair(other_group, 4).clone();
    let false_death = ReplayPair {
        interval: group.interval,
        birth: a_pair.birth,
        death: None,
    };
    for pair in [false_birth, false_death] {
        let bytes = encode_artifact_claim(ArtifactClaim {
            source: &source,
            vertex_count: 12,
            threshold: None,
            modulus: 5,
            interval: group.interval,
            scale: group.scale,
            group_id: group.id,
            class_id: group.class_ids[basis_index],
            basis_index,
            basis,
            pair: &pair,
            cycle: &cycle,
            chain: &[],
        });
        assert_current_digest(&bytes);
        assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
    }
}

#[test]
fn resealed_false_birth_and_death_are_rejected_with_recomputed_ids() {
    let (source, replay) = overlapping_replay(None);
    let group = group_with_interval(&replay, 1.0, 3.0);
    let basis_index = component_basis_index(group, 0);
    let basis = &group.basis[basis_index];
    let pair = component_pair(group, 0);
    let cycle = normalized_square_cycle_for_basis(group, basis_index, 0, 5);
    for (birth, death) in [(2.0, 3.0), (1.0, 4.0)] {
        let interval = crate::proof::ProofBar {
            dimension: 1,
            birth,
            death,
        };
        let group_id = independent_group_id(interval, 5, group.scale, &group.basis);
        let class_id = independent_class_id(group_id, basis_index, group.scale, basis);
        let chain = square_chain(0);
        let bytes = encode_artifact_claim(ArtifactClaim {
            source: &source,
            vertex_count: 12,
            threshold: None,
            modulus: 5,
            interval,
            scale: group.scale,
            group_id,
            class_id,
            basis_index,
            basis,
            pair,
            cycle: &cycle,
            chain: &chain,
        });
        assert_current_digest(&bytes);
        assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
    }
}

#[test]
fn resealed_finite_broken_chain_is_rejected_after_digest_recomputation() {
    let mut bytes = component_artifact(None, 1.0, 3.0, 0);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_ok());
    let payload_len = bytes.len() - 32;
    put_u32_at(&mut bytes, payload_len - 4, 2);
    reseal(&mut bytes);
    assert_current_digest(&bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn essentiality_is_relative_to_the_threshold() {
    let thresholded = component_artifact(Some(2.0), 1.0, f64::INFINITY, 0);
    let checked = verify_persistent_class(&thresholded, ProofLimits::default()).unwrap();
    assert!(checked.interval().death.is_infinite());
    assert!(checked.bounding_chain().is_empty());

    let unthresholded = component_artifact(None, 1.0, 3.0, 0);
    let checked = verify_persistent_class(&unthresholded, ProofLimits::default()).unwrap();
    assert_eq!(checked.interval().death.to_bits(), 3.0f64.to_bits());
}

#[test]
fn threshold_below_birth_rejects_a_resealed_essential_claim() {
    let (source, replay) = overlapping_replay(Some(2.0));
    let group = group_with_interval(&replay, 1.0, f64::INFINITY);
    let basis_index = component_basis_index(group, 0);
    let basis = &group.basis[basis_index];
    let pair = component_pair(group, 0);
    let cycle = normalized_square_cycle_for_basis(group, basis_index, 0, 5);
    let bytes = encode_artifact_claim(ArtifactClaim {
        source: &source,
        vertex_count: 12,
        threshold: Some(0.5),
        modulus: 5,
        interval: group.interval,
        scale: group.scale,
        group_id: group.id,
        class_id: group.class_ids[basis_index],
        basis_index,
        basis,
        pair,
        cycle: &cycle,
        chain: &[],
    });
    assert_current_digest(&bytes);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}
