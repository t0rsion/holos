use sha2::{Digest, Sha256};

use super::*;
use crate::{
    CertificateLimits, CircularCoordinateParams, CohomologyLimits, PersistentClassArtifact,
    RipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{CircularProofLimits, verify_persistent_coordinate};

fn cycle_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (0, 7, 1.0),
        ],
    )
    .unwrap()
}

fn finite_square_graph_with_triangle_and_isolate() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 1.0),
            (1, 4, 1.0),
            (0, 2, 2.0),
        ],
    )
    .unwrap()
}

fn class_artifact(modulus: u32) -> PersistentClassArtifact {
    let graph = cycle_graph();
    PersistentClassArtifact::build(
        &graph,
        &RipsParams::new(1).with_modulus(modulus),
        0,
        0,
        CertificateLimits::default(),
    )
    .unwrap()
}

#[test]
fn selected_coordinate_handoff_is_checked_without_a_space_identifier() {
    let class = class_artifact(47);
    let coordinate =
        PersistentCoordinateArtifact::build(&class, CircularCoordinateParams::default(), None)
            .unwrap();
    assert_eq!(coordinate.interval(), class.class().interval);
    assert_eq!(coordinate.cycle(), class.cycle());
    assert_eq!(coordinate.potential().len(), class.source().len());
    assert_eq!(coordinate.phase().len(), class.source().len());
    let bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    assert!(bytes.starts_with(b"HOLOSPH\0"));
    verify_persistent_coordinate(&bytes, CircularProofLimits::default()).unwrap();
}

#[test]
fn selected_coordinate_ignores_full_space_limits() {
    let class = class_artifact(47);
    let params = CircularCoordinateParams::default().with_cohomology_limits(CohomologyLimits {
        max_vertices: 0,
        ..CohomologyLimits::default()
    });
    let coordinate = PersistentCoordinateArtifact::build(&class, params, None).unwrap();
    assert_eq!(coordinate.potential().len(), class.source().len());
}

#[test]
fn supplied_modulus_two_lift_is_checked() {
    let class = class_artifact(2);
    let lift = class
        .class()
        .cocycle
        .terms
        .iter()
        .map(|term| crate::IntegralCocycleTerm {
            u: term.u,
            v: term.v,
            coefficient: 1,
        })
        .collect::<Vec<_>>();
    let coordinate = PersistentCoordinateArtifact::build(
        &class,
        CircularCoordinateParams::default(),
        Some(&lift),
    )
    .unwrap();
    let bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    verify_persistent_coordinate(&bytes, CircularProofLimits::default()).unwrap();
}

#[test]
fn checker_rejects_resealed_lift_and_potential_mutations() {
    let class = class_artifact(47);
    let coordinate =
        PersistentCoordinateArtifact::build(&class, CircularCoordinateParams::default(), None)
            .unwrap();
    let bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    let offsets = offsets(&bytes);

    let mut multiplier = bytes.clone();
    put_u32_at(&mut multiplier, offsets.multiplier, 0);
    reseal(&mut multiplier);
    assert!(verify_persistent_coordinate(&multiplier, CircularProofLimits::default()).is_err());

    let mut lift = bytes.clone();
    let coefficient_offset = offsets.integral_start + 16;
    let coefficient = read_i64_at(&lift, coefficient_offset);
    put_i64_at(&mut lift, coefficient_offset, coefficient + 1);
    reseal(&mut lift);
    assert!(verify_persistent_coordinate(&lift, CircularProofLimits::default()).is_err());

    let mut divisibility = bytes.clone();
    let declared = read_u64_at(&divisibility, offsets.divisibility);
    put_u64_at(&mut divisibility, offsets.divisibility, declared + 1);
    reseal(&mut divisibility);
    assert!(verify_persistent_coordinate(&divisibility, CircularProofLimits::default()).is_err());

    let mut potential = bytes;
    put_u64_at(
        &mut potential,
        offsets.potential_start + 8,
        0.25f64.to_bits(),
    );
    reseal(&mut potential);
    assert!(verify_persistent_coordinate(&potential, CircularProofLimits::default()).is_err());
}

#[test]
fn checker_rejects_a_resealed_nested_source_mutation() {
    let class = class_artifact(47);
    let coordinate =
        PersistentCoordinateArtifact::build(&class, CircularCoordinateParams::default(), None)
            .unwrap();
    let mut bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    let (nested_start, _) = nested_bounds(&bytes);
    put_u64_at(&mut bytes, nested_start + 31 + 16, 3.0f64.to_bits());
    reseal_nested(&mut bytes);
    assert!(verify_persistent_coordinate(&bytes, CircularProofLimits::default()).is_err());
}

#[test]
fn checker_accepts_a_valid_nonprimitive_rescaling() {
    let class = class_artifact(47);
    let coordinate =
        PersistentCoordinateArtifact::build(&class, CircularCoordinateParams::default(), None)
            .unwrap();
    let mut bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    let offsets = offsets(&bytes);
    let factor = 2u64;
    let modulus = u64::from(coordinate.modulus());
    let multiplier = (u64::from(coordinate.field_multiplier()) * factor % modulus) as u32;
    assert_ne!(multiplier, 0);
    put_u32_at(&mut bytes, offsets.multiplier, multiplier);
    put_u64_at(
        &mut bytes,
        offsets.divisibility,
        coordinate.divisibility() * factor,
    );
    let integral_count = read_u64_at(&bytes, offsets.multiplier + 12) as usize;
    for index in 0..integral_count {
        let coefficient_offset = offsets.integral_start + index * 24 + 16;
        let coefficient = read_i64_at(&bytes, coefficient_offset);
        put_i64_at(&mut bytes, coefficient_offset, coefficient * factor as i64);
    }
    let potential_count =
        read_u64_at(&bytes, offsets.integral_start + integral_count * 24) as usize;
    for index in 0..potential_count {
        let potential_offset = offsets.potential_start + index * 8;
        let potential = f64::from_bits(read_u64_at(&bytes, potential_offset));
        put_u64_at(
            &mut bytes,
            potential_offset,
            (potential * factor as f64).to_bits(),
        );
    }
    reseal(&mut bytes);
    let checked = verify_persistent_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.divisibility(), coordinate.divisibility() * factor);
}

#[test]
fn active_triangles_and_isolated_vertices_replay_with_the_checked_coordinate() {
    let class = PersistentClassArtifact::build(
        &finite_square_graph_with_triangle_and_isolate(),
        &RipsParams::new(1).with_threshold(2.0).with_modulus(47),
        0,
        0,
        CertificateLimits::default(),
    )
    .unwrap();
    let coordinate =
        PersistentCoordinateArtifact::build(&class, CircularCoordinateParams::default(), None)
            .unwrap();
    assert!(coordinate.interval().death.is_finite());
    assert!(
        coordinate
            .source()
            .edges()
            .any(|(u, v, value)| (u, v) == (0, 1) && value <= coordinate.scale())
    );
    assert!(
        coordinate
            .source()
            .edges()
            .any(|(u, v, value)| (u, v) == (0, 4) && value <= coordinate.scale())
    );
    assert!(
        coordinate
            .source()
            .edges()
            .any(|(u, v, value)| (u, v) == (1, 4) && value <= coordinate.scale())
    );
    assert!(
        coordinate
            .source()
            .edges()
            .any(|(u, v, value)| (u, v) == (0, 2) && value > coordinate.scale())
    );
    assert_eq!(coordinate.potential().len(), 6);
    assert_eq!(coordinate.potential()[5].to_bits(), 0.0f64.to_bits());
    let bytes = coordinate.encode(CertificateLimits::default()).unwrap();
    let checked = verify_persistent_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.potential().len(), 6);
    assert_eq!(checked.potential()[5].to_bits(), 0.0f64.to_bits());
}

struct Offsets {
    multiplier: usize,
    divisibility: usize,
    integral_start: usize,
    potential_start: usize,
}

fn offsets(bytes: &[u8]) -> Offsets {
    let nested_count = read_u64_at(bytes, 19);
    let multiplier = 27 + usize::try_from(nested_count).unwrap();
    let divisibility = multiplier + 4;
    let integral_count = read_u64_at(bytes, multiplier + 12);
    let integral_start = multiplier + 20;
    let integral_count = usize::try_from(integral_count).unwrap();
    let potential_count = read_u64_at(bytes, integral_start + integral_count * 24);
    let potential_start = integral_start + integral_count * 24 + 8;
    assert_eq!(potential_count as usize, cycle_graph().len());
    Offsets {
        multiplier,
        divisibility,
        integral_start,
        potential_start,
    }
}

fn nested_bounds(bytes: &[u8]) -> (usize, usize) {
    let start = 27;
    let count = usize::try_from(read_u64_at(bytes, 19)).unwrap();
    (start, start + count)
}

fn reseal_nested(bytes: &mut [u8]) {
    let (start, end) = nested_bounds(bytes);
    let payload_end = end - 32;
    let digest: [u8; 32] = Sha256::digest(&bytes[start..payload_end]).into();
    bytes[payload_end..end].copy_from_slice(&digest);
    reseal(bytes);
}

fn reseal(bytes: &mut [u8]) {
    let payload_len = bytes.len() - 32;
    let digest: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    bytes[payload_len..].copy_from_slice(&digest);
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn read_i64_at(bytes: &[u8], offset: usize) -> i64 {
    i64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn put_u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn put_i64_at(bytes: &mut [u8], offset: usize, value: i64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}

fn put_u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}
