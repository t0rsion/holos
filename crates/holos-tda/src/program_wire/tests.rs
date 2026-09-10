use crate::{CertificateLimits, RipsParams, SparseDistanceMatrix};
use proptest::prelude::*;

use super::*;

fn graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, 4.5),
        ],
    )
    .unwrap()
}

fn tree_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 2.0), (2, 3, 3.0)]).unwrap()
}

#[test]
fn program_round_trips_and_verifies_without_the_solver() {
    let input = graph();
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let artifact =
            ProgramArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
        let bytes = artifact.encode().unwrap();
        let decoded = ProgramArtifact::decode(
            &bytes,
            ProgramDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        let program = decoded
            .verify(&input, CertificateLimits::default())
            .unwrap();
        assert!(diagram_bits_equal(
            &program.result().diagram,
            artifact.diagram()
        ));
    }
}

#[test]
fn program_rejects_invalid_modulus_and_negative_zero_threshold() {
    let input = tree_graph();
    let artifact =
        ProgramArtifact::build(&input, &RipsParams::new(1), CertificateLimits::default()).unwrap();
    let bytes = artifact.encode().unwrap();

    let modulus_offset = MAGIC.len() + 2 + 1;
    for modulus in [0u32, 1, 4, 32_768] {
        let mut changed = bytes.clone();
        changed[modulus_offset..modulus_offset + 4].copy_from_slice(&modulus.to_be_bytes());
        assert!(
            ProgramArtifact::decode(
                &changed,
                ProgramDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err(),
            "modulus {modulus} must be rejected"
        );
    }

    let negative_threshold = ProgramArtifact::build(
        &input,
        &RipsParams::new(1).with_threshold(-0.0),
        CertificateLimits::default(),
    );
    assert!(negative_threshold.is_err());

    let threshold_artifact = ProgramArtifact::build(
        &input,
        &RipsParams::new(1).with_threshold(0.0),
        CertificateLimits::default(),
    )
    .unwrap();
    let mut threshold_bytes = threshold_artifact.encode().unwrap();
    let threshold_tag_offset = MAGIC.len() + 2 + 1 + 4 + 8;
    assert_eq!(threshold_bytes[threshold_tag_offset], 1);
    let threshold_offset = threshold_tag_offset + 1;
    threshold_bytes[threshold_offset..threshold_offset + 8]
        .copy_from_slice(&(-0.0f64).to_bits().to_be_bytes());
    assert!(
        ProgramArtifact::decode(
            &threshold_bytes,
            ProgramDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    );

    assert!(!artifact.diagram().bars.is_empty());
    let first_bar_offset = MAGIC.len() + 2 + 1 + 4 + 8 + 1 + 8 + 8 + 32;
    for scalar_offset in [first_bar_offset + 8, first_bar_offset + 16] {
        let mut negative_zero_bar = bytes.clone();
        negative_zero_bar[scalar_offset..scalar_offset + 8]
            .copy_from_slice(&(-0.0f64).to_bits().to_be_bytes());
        assert!(
            ProgramArtifact::decode(
                &negative_zero_bar,
                ProgramDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn mutation_wrong_input_and_limits_are_rejected() {
    let input = graph();
    let params = RipsParams::new(1);
    let artifact = ProgramArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
    let bytes = artifact.encode().unwrap();
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    assert!(
        ProgramArtifact::decode(
            &changed,
            ProgramDecodeLimits::default(),
            CertificateLimits::default()
        )
        .is_err()
    );
    assert!((0..bytes.len()).all(|end| {
        ProgramArtifact::decode(
            &bytes[..end],
            ProgramDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    }));
    let other = SparseDistanceMatrix::from_triplets(
        7,
        &[(0, 1, 1.0), (1, 2, 2.0), (2, 3, 3.0), (0, 3, 5.0)],
    )
    .unwrap();
    assert!(
        artifact
            .verify(&other, CertificateLimits::default())
            .is_err()
    );
    let limits = ProgramDecodeLimits {
        max_atoms: 1,
        ..ProgramDecodeLimits::default()
    };
    assert!(ProgramArtifact::decode(&bytes, limits, CertificateLimits::default()).is_err());
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = ProgramArtifact::decode(
            &bytes,
            ProgramDecodeLimits::default(),
            CertificateLimits::default(),
        );
    }
}
