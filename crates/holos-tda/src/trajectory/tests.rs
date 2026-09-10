use crate::{AtlasDecodeLimits, CertificateLimits, RipsParams, SparseDistanceMatrix, UpdateMode};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

use super::*;

fn graph(weights: [f64; 6]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, weights[0]),
            (0, 2, weights[1]),
            (0, 3, weights[2]),
            (1, 2, weights[3]),
            (1, 3, weights[4]),
            (2, 3, weights[5]),
        ],
    )
    .unwrap()
}

#[test]
fn trajectory_round_trips_reuse_and_region_change() {
    let initial = graph([1.0, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let reuse = graph([1.01, 2.01, 1.11, 1.21, 2.11, 1.31]);
    let change = graph([2.2, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let artifact = TrajectoryArtifact::build(
        &initial,
        &[reuse, change],
        &RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.steps[0].mode, UpdateMode::Reused);
    assert_eq!(artifact.steps[1].mode, UpdateMode::Recomputed);
    let bytes = artifact.encode().unwrap();
    assert_eq!(
        bytes.len(),
        2_354,
        "the canonical trajectory fixture must retain its wire size"
    );
    assert_eq!(
        Sha256::digest(&bytes).as_slice(),
        [
            0xc4, 0xdc, 0x1c, 0xd0, 0x5f, 0xb4, 0x13, 0x16, 0xe8, 0xa6, 0x9d, 0x0e, 0x7d, 0xaa,
            0x7d, 0xe7, 0x27, 0x99, 0x31, 0xd5, 0x73, 0x61, 0xab, 0xfb, 0x35, 0x25, 0xdc, 0x91,
            0x49, 0x7c, 0x29, 0xc9,
        ]
    );
    let decoded = TrajectoryArtifact::decode(
        &bytes,
        TrajectoryDecodeLimits::default(),
        AtlasDecodeLimits::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    let verified = decoded.verify(CertificateLimits::default()).unwrap();
    assert_eq!(verified.steps[0].mode, UpdateMode::Reused);
    assert_eq!(verified.steps[1].mode, UpdateMode::Recomputed);
}

#[test]
fn changed_event_and_truncation_are_rejected() {
    let initial = graph([1.0, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let change = graph([2.2, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let mut artifact = TrajectoryArtifact::build(
        &initial,
        &[change],
        &RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    artifact.steps[0].events[0].old_first = Some(7.0);
    assert!(artifact.verify(CertificateLimits::default()).is_err());

    let bytes = TrajectoryArtifact::build(
        &initial,
        &[],
        &RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap()
    .encode()
    .unwrap();
    for end in 0..bytes.len() {
        assert!(
            TrajectoryArtifact::decode(
                &bytes[..end],
                TrajectoryDecodeLimits::default(),
                AtlasDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        );
    }
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = TrajectoryArtifact::decode(
            &bytes,
            TrajectoryDecodeLimits::default(),
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        );
    }
}
