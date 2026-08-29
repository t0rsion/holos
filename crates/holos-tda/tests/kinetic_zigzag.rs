#![cfg(holos_repository_tests)]

use holos_tda::{
    KineticEdge, KineticFiltration, KineticLimits, KineticZigzagArtifact,
    KineticZigzagArtifactLimits,
};
use holos_tda_check::{ProofLimits, verify_kinetic_zigzag};

fn filling_trajectory() -> KineticFiltration {
    let mut edges = Vec::new();
    for u in 0..6 {
        for v in u + 1..6 {
            if u / 2 != v / 2 {
                edges.push(KineticEdge {
                    u,
                    v,
                    intercept: 0.5,
                    velocity: 0.0,
                });
            }
        }
    }
    edges.push(KineticEdge {
        u: 0,
        v: 1,
        intercept: 2.0,
        velocity: -2.0,
    });
    KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap()
}

#[test]
fn independent_checker_rebuilds_an_h2_kinetic_zigzag() {
    let limits = KineticZigzagArtifactLimits::default();
    let (artifact, zigzag) =
        KineticZigzagArtifact::build(&filling_trajectory(), 2, 1.0, 5, limits).unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let checked = verify_kinetic_zigzag(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.dimension, 2);
    assert_eq!(checked.nodes, zigzag.nodes.len());
    assert_eq!(checked.arrows, zigzag.arrows.len());
    assert_eq!(checked.intervals, zigzag.barcode.intervals.len());
}

#[test]
fn independent_checker_rejects_mutations_and_short_inputs() {
    let limits = KineticZigzagArtifactLimits::default();
    let (artifact, _) =
        KineticZigzagArtifact::build(&filling_trajectory(), 2, 1.0, 3, limits).unwrap();
    let bytes = artifact.encode(limits).unwrap();
    for position in [0, 9, bytes.len() / 2, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[position] ^= 0x80;
        let result =
            std::panic::catch_unwind(|| verify_kinetic_zigzag(&changed, ProofLimits::default()));
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
    for length in 0..bytes.len().min(256) {
        let result = std::panic::catch_unwind(|| {
            verify_kinetic_zigzag(&bytes[..length], ProofLimits::default())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
