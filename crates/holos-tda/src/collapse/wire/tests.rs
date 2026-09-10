use super::*;

use crate::DistanceMatrix;
use crate::collapse::verify::verify_dense_artifact;
use crate::collapse::{
    AdaptiveCollapseParams, CollapseObjective, collapse_dense, collapse_dense_adaptive,
};
use proptest::prelude::*;

fn k4() -> DistanceMatrix {
    DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
}

#[test]
fn every_certificate_version_round_trips_canonically() {
    let results = [
        collapse_dense(&k4(), None).unwrap(),
        crate::collapse::collapse_dense_rounds_parallel(&k4(), None, 2).unwrap(),
        collapse_dense_adaptive(
            &k4(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H2),
        )
        .unwrap(),
    ];
    for result in results {
        let artifact = CollapseArtifact::from_result(&result).unwrap();
        let bytes = artifact.encode().unwrap();
        let decoded = CollapseArtifact::decode(&bytes, DecodeLimits::default()).unwrap();
        assert_eq!(decoded, artifact);
        assert_eq!(decoded.encode().unwrap(), bytes);
    }
}

#[test]
fn truncation_corruption_and_trailing_bytes_are_rejected() {
    let result = collapse_dense_adaptive(
        &k4(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H1),
    )
    .unwrap();
    let bytes = CollapseArtifact::from_result(&result)
        .unwrap()
        .encode()
        .unwrap();
    for end in 0..bytes.len() {
        assert!(CollapseArtifact::decode(&bytes[..end], DecodeLimits::default()).is_err());
    }
    let mut corrupt = bytes.clone();
    // The input binding starts after the fixed fields of this envelope.
    corrupt[59] ^= 1;
    assert!(CollapseArtifact::decode(&corrupt, DecodeLimits::default()).is_err());

    let mut trailing = bytes;
    trailing.push(0);
    let error = CollapseArtifact::decode(&trailing, DecodeLimits::default()).unwrap_err();
    assert!(error.message().contains("trailing bytes"));
}

#[test]
fn noncanonical_output_records_are_rejected() {
    let matrix =
        crate::SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 2.0)]).unwrap();
    let result = crate::collapse::collapse_sparse(&matrix, None).unwrap();
    let bytes = CollapseArtifact::from_result(&result)
        .unwrap()
        .encode()
        .unwrap();

    // The two 24-byte output records start after the 139-byte header.
    let mut permuted = bytes.clone();
    let (first, second) = permuted[139..187].split_at_mut(24);
    first.swap_with_slice(second);
    assert!(
        CollapseArtifact::decode(&permuted, DecodeLimits::default())
            .unwrap_err()
            .message()
            .contains("ascending endpoint order")
    );

    let mut negative_zero = bytes;
    negative_zero[155..163].copy_from_slice(&(-0.0f64).to_bits().to_be_bytes());
    assert!(
        CollapseArtifact::decode(&negative_zero, DecodeLimits::default())
            .unwrap_err()
            .message()
            .contains("negative zero")
    );
}

#[test]
fn byte_and_collection_limits_apply_before_success() {
    let result = collapse_dense(&k4(), None).unwrap();
    let bytes = CollapseArtifact::from_result(&result)
        .unwrap()
        .encode()
        .unwrap();
    let limits = DecodeLimits {
        max_bytes: bytes.len() - 1,
        ..DecodeLimits::default()
    };
    assert!(CollapseArtifact::decode(&bytes, limits).is_err());

    let limits = DecodeLimits {
        max_vertices: 3,
        ..DecodeLimits::default()
    };
    assert!(CollapseArtifact::decode(&bytes, limits).is_err());

    let limits = DecodeLimits {
        max_steps: 0,
        ..DecodeLimits::default()
    };
    assert!(CollapseArtifact::decode(&bytes, limits).is_err());
}

#[test]
fn independent_verifier_checks_the_input_binding_and_certificate() {
    let result = collapse_dense_adaptive(
        &k4(),
        None,
        AdaptiveCollapseParams::new(CollapseObjective::H2),
    )
    .unwrap();
    let artifact = CollapseArtifact::decode(
        &CollapseArtifact::from_result(&result)
            .unwrap()
            .encode()
            .unwrap(),
        DecodeLimits::default(),
    )
    .unwrap();
    verify_dense_artifact(&k4(), None, &artifact).unwrap();

    let other = DistanceMatrix::from_condensed(vec![2.0; 6]).unwrap();
    let error = verify_dense_artifact(&other, None, &artifact).unwrap_err();
    assert!(
        error.message.contains("artifact binding"),
        "{}",
        error.message
    );

    let mut changed_witness = artifact.clone();
    changed_witness.certificate.steps[0].witnesses[0].1 = 0;
    let error = verify_dense_artifact(&k4(), None, &changed_witness).unwrap_err();
    assert!(error.message.contains("apex"), "{}", error.message);
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = CollapseArtifact::decode(&bytes, DecodeLimits::default());
    }
}
