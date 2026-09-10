#![cfg(holos_repository_tests)]

use holos_tda::{
    Bigrade, BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceLimits,
    BipersistenceModule, DegreeRipsBifiltration, DegreeRipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{BipersistenceProofLimits, verify_bipersistence};
use sha2::{Digest, Sha256};

#[test]
fn every_unweighted_four_vertex_module_matches_the_dense_oracle() {
    for mask in 0u64..1 << 6 {
        check_graph(4, graph_from_binary_mask(4, mask));
    }
}

#[test]
#[ignore = "exhaustive 3^6 weighted graph sweep; run with --ignored in release"]
fn every_two_level_four_vertex_module_matches_the_dense_oracle() {
    for code in 0u64..3u64.pow(6) {
        check_graph(4, graph_from_ternary_code(4, code));
    }
}

fn check_graph(vertex_count: usize, edges: Vec<(usize, usize, f64)>) {
    let graph = SparseDistanceMatrix::from_triplets(vertex_count, &edges).unwrap();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let module =
        BipersistenceModule::from_degree_rips(&degree_rips, 2, BipersistenceLimits::default())
            .unwrap();
    for node in module.nodes() {
        let complex = direct_slice(
            vertex_count,
            &edges,
            module.scales()[node.grade.scale()],
            module.minimum_degrees()[node.grade.density()],
        );
        assert_eq!(node.rank, direct_h1_rank(vertex_count, &complex));
    }
    for lower_scale in 0..module.scales().len() {
        for lower_density in 0..module.minimum_degrees().len() {
            let lower_grade = Bigrade::new(lower_scale, lower_density);
            let lower = direct_slice(
                vertex_count,
                &edges,
                module.scales()[lower_scale],
                module.minimum_degrees()[lower_density],
            );
            for upper_scale in lower_scale..module.scales().len() {
                for upper_density in lower_density..module.minimum_degrees().len() {
                    let upper_grade = Bigrade::new(upper_scale, upper_density);
                    let upper = direct_slice(
                        vertex_count,
                        &edges,
                        module.scales()[upper_scale],
                        module.minimum_degrees()[upper_density],
                    );
                    assert_eq!(
                        module.map_rank(lower_grade, upper_grade).unwrap(),
                        direct_inclusion_rank(vertex_count, &lower, &upper),
                        "map {:?} to {:?} differs on {edges:?}",
                        lower_grade,
                        upper_grade,
                    );
                }
            }
        }
    }
}

#[test]
fn generalized_ranks_match_a_direct_limit_colimit_oracle() {
    let edges = vec![
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
        (0, 2, 2.0),
        (1, 3, 2.0),
    ];
    let graph = SparseDistanceMatrix::from_triplets(4, &edges).unwrap();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let module =
        BipersistenceModule::from_degree_rips(&degree_rips, 2, BipersistenceLimits::default())
            .unwrap();

    let mut checked_nontrivial = false;
    for lower_scale in 0..module.scales().len() {
        for lower_density in 0..module.minimum_degrees().len() {
            for upper_scale in lower_scale..module.scales().len() {
                for upper_density in lower_density..module.minimum_degrees().len() {
                    let lower = Bigrade::new(lower_scale, lower_density);
                    let upper = Bigrade::new(upper_scale, upper_density);
                    let rectangle =
                        holos_tda::bipersistence::BipersistenceRectangle::new(lower, upper)
                            .unwrap();
                    let expected = direct_rectangle_rank(
                        4,
                        &edges,
                        &module.scales()[lower_scale..=upper_scale],
                        &module.minimum_degrees()[lower_density..=upper_density],
                    );
                    assert_eq!(
                        module.rectangle_rank(rectangle).unwrap(),
                        expected,
                        "rectangle {:?} to {:?} differs",
                        lower,
                        upper,
                    );
                    if upper_scale > lower_scale && upper_density > lower_density {
                        checked_nontrivial = true;
                    }
                }
            }
        }
    }
    assert!(checked_nontrivial);
}

#[test]
fn connected_nonrectangular_region_matches_the_direct_oracle() {
    let edges = vec![
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
        (0, 2, 2.0),
        (1, 3, 2.0),
    ];
    let graph = SparseDistanceMatrix::from_triplets(4, &edges).unwrap();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let module =
        BipersistenceModule::from_degree_rips(&degree_rips, 2, BipersistenceLimits::default())
            .unwrap();
    let region = holos_tda::bipersistence::BipersistenceRegion::new(vec![
        Bigrade::new(0, 1),
        Bigrade::new(1, 0),
        Bigrade::new(1, 1),
        Bigrade::new(1, 2),
        Bigrade::new(2, 1),
    ])
    .unwrap();
    assert!(region.grades().windows(2).all(|pair| pair[0] != pair[1]));
    assert!(!region.grades().iter().any(|candidate| {
        region
            .grades()
            .iter()
            .all(|other| candidate.precedes(*other))
    }));
    assert!(!region.grades().iter().any(|candidate| {
        region
            .grades()
            .iter()
            .all(|other| other.precedes(*candidate))
    }));

    assert_eq!(
        module.region_rank(&region).unwrap(),
        direct_region_rank(
            4,
            &edges,
            module.scales(),
            module.minimum_degrees(),
            region.grades(),
        )
    );
}

#[test]
fn checker_rejects_resealed_axis_and_map_tampering() {
    let bytes = artifact_bytes();
    let layout = artifact_layout(&bytes);
    let limits = BipersistenceProofLimits::default();

    let mut scale_tampered = bytes.clone();
    write_u64(
        &mut scale_tampered,
        layout.scale_start + 8,
        0.5f64.to_bits(),
    );
    reseal(&mut scale_tampered);
    assert!(verify_bipersistence(&scale_tampered, limits).is_err());

    let mut density_tampered = bytes.clone();
    write_u64(&mut density_tampered, layout.density_start + 8, 3);
    reseal(&mut density_tampered);
    assert!(verify_bipersistence(&density_tampered, limits).is_err());

    let mut map_tampered = bytes;
    write_u64(&mut map_tampered, layout.first_map_rank, 1);
    reseal(&mut map_tampered);
    assert!(verify_bipersistence(&map_tampered, limits).is_err());
}

fn artifact_bytes() -> Vec<u8> {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let (artifact, _) =
        BipersistenceArtifact::build(&degree_rips, 2, BipersistenceArtifactLimits::default())
            .unwrap();
    artifact
        .encode(BipersistenceArtifactLimits::default())
        .unwrap()
}

struct ArtifactLayout {
    scale_start: usize,
    density_start: usize,
    first_map_rank: usize,
}

fn artifact_layout(bytes: &[u8]) -> ArtifactLayout {
    let mut offset = 11;
    offset += 8;
    let edge_count = read_u64(bytes, offset) as usize;
    offset += 8 + edge_count * 24;
    offset += 8;
    offset += 4;
    let scale_count = read_u64(bytes, offset) as usize;
    offset += 8;
    let scale_start = offset;
    offset += scale_count * 8;
    let density_count = read_u64(bytes, offset) as usize;
    offset += 8;
    let density_start = offset;
    offset += density_count * 8;
    let node_count = read_u64(bytes, offset) as usize;
    offset += 8 + node_count * 56;
    let map_count = read_u64(bytes, offset);
    assert!(map_count > 0);
    offset += 8;
    let first_map_rank = offset + 16 + 16 + 32 + 32;
    ArtifactLayout {
        scale_start,
        density_start,
        first_map_rank,
    }
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}

fn reseal(bytes: &mut [u8]) {
    let split = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-bipersistence-v1");
    hash.update(&bytes[..split]);
    let digest: [u8; 32] = hash.finalize().into();
    bytes[split..].copy_from_slice(&digest);
}

#[path = "bipersistence/oracle.rs"]
mod oracle;
use oracle::*;
