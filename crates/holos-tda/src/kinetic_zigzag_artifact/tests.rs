use super::*;

fn trajectory() -> KineticFiltration {
    KineticFiltration::new(
        4,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.5,
                velocity: 1.0,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 0.5,
                velocity: 0.0,
            },
            KineticEdge {
                u: 2,
                v: 3,
                intercept: 0.5,
                velocity: 0.0,
            },
            KineticEdge {
                u: 0,
                v: 3,
                intercept: 0.5,
                velocity: 0.0,
            },
        ],
        0.0,
        1.0,
        KineticLimits::default(),
    )
    .unwrap()
}

#[test]
fn artifact_round_trips_and_replays_the_complete_zigzag() {
    let limits = KineticZigzagArtifactLimits::default();
    let (artifact, zigzag) =
        KineticZigzagArtifact::build(&trajectory(), 1, 1.0, 3, limits).unwrap();
    let bytes = artifact.encode(limits).unwrap();
    let decoded = KineticZigzagArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    assert_eq!(decoded.summary().nodes, zigzag.nodes.len());
    assert!(!decoded.intervals().is_empty());
}

#[test]
fn mutations_and_truncations_are_rejected() {
    let limits = KineticZigzagArtifactLimits::default();
    let (artifact, _) = KineticZigzagArtifact::build(&trajectory(), 1, 1.0, 5, limits).unwrap();
    let bytes = artifact.encode(limits).unwrap();
    for position in [0, 8, bytes.len() / 2, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[position] ^= 0x40;
        assert!(KineticZigzagArtifact::decode(&changed, limits).is_err());
    }
    for length in 0..bytes.len().min(128) {
        assert!(KineticZigzagArtifact::decode(&bytes[..length], limits).is_err());
    }
}
