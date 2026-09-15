use super::codec::{put_optional_f64, put_u16, put_u32, put_u64, put_usize};
use super::*;
use crate::{CertificateLimits, RipsParams, SparseDistanceMatrix};
use proptest::prelude::*;

fn square() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
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
    .unwrap()
}

#[test]
fn atlas_round_trips_and_verifies_without_the_solver() {
    let input = square();
    let params = RipsParams::new(1).with_modulus(3);
    let artifact = AtlasArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
    let bytes = artifact.encode().unwrap();
    assert_eq!(u16::from_be_bytes(bytes[8..10].try_into().unwrap()), 2);
    let decoded = AtlasArtifact::decode(
        &bytes,
        AtlasDecodeLimits::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    let atlas = decoded
        .verify(&input, CertificateLimits::default())
        .unwrap();
    assert_eq!(atlas.explained().spaces, artifact.spaces());
}

#[test]
fn atlas_wire_provenance_mutations_are_rejected() {
    let input = square();
    let artifact = AtlasArtifact::build(
        &input,
        &RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    let bytes = artifact.encode().unwrap();
    let class = &artifact.spaces()[0].basis[0];
    let provenance = class.provenance.as_ref().unwrap();
    let source_offset = bytes
        .windows(32)
        .position(|window| window == provenance.source_graph_digest())
        .unwrap();
    let class_offset = source_offset
        + 32
        + bytes[source_offset + 32..]
            .windows(32)
            .position(|window| window == provenance.class_digest())
            .unwrap();
    assert!(source_offset > 0);
    assert!(class_offset > source_offset);

    let mut changed_source = bytes.clone();
    changed_source[source_offset] ^= 1;
    let decoded = AtlasArtifact::decode(
        &changed_source,
        AtlasDecodeLimits::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert!(
        decoded
            .verify(&input, CertificateLimits::default())
            .is_err()
    );

    let mut changed_tag = bytes.clone();
    changed_tag[source_offset - 1] = 2;
    assert!(
        AtlasArtifact::decode(
            &changed_tag,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    );

    let mut changed_class_digest_wire = bytes.clone();
    changed_class_digest_wire[class_offset] ^= 1;
    assert!(
        AtlasArtifact::decode(
            &changed_class_digest_wire,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    );

    let mut changed_class = artifact.clone();
    let class = &mut changed_class.explained.spaces[0].basis[0];
    let (source_graph_digest, class_digest, interval, modulus, scale) = {
        let provenance = class.provenance.as_ref().unwrap();
        (
            *provenance.source_graph_digest(),
            *provenance.class_digest(),
            provenance.interval(),
            provenance.modulus(),
            provenance.scale(),
        )
    };
    let mut class_digest = class_digest;
    class_digest[0] ^= 1;
    class.provenance = Some(crate::PersistentClassProvenance::from_parts(
        source_graph_digest,
        class_digest,
        interval,
        modulus,
        scale,
    ));
    assert!(changed_class.encode().is_err());

    let mut old_version = bytes.clone();
    old_version[8..10].copy_from_slice(&1u16.to_be_bytes());
    assert!(
        AtlasArtifact::decode(
            &old_version,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    );
}

#[test]
fn mutations_limits_and_wrong_inputs_are_rejected() {
    let input = square();
    let params = RipsParams::new(1);
    let artifact = AtlasArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
    let bytes = artifact.encode().unwrap();
    for end in 0..bytes.len() {
        assert!(
            AtlasArtifact::decode(
                &bytes[..end],
                AtlasDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        );
    }
    let limits = AtlasDecodeLimits {
        max_bytes: bytes.len() - 1,
        ..AtlasDecodeLimits::default()
    };
    assert!(AtlasArtifact::decode(&bytes, limits, CertificateLimits::default()).is_err());

    let other =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    assert!(
        artifact
            .verify(&other, CertificateLimits::default())
            .is_err()
    );

    let mut changed_pair = artifact.clone();
    changed_pair.explained.spaces[0].critical_pairs[0]
        .birth
        .vertices = vec![2, 3];
    assert!(
        changed_pair
            .verify(&input, CertificateLimits::default())
            .is_err()
    );
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = AtlasArtifact::decode(
            &bytes,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        );
    }
}

fn atlas_with_space_counts(basis: usize, critical_pairs: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    put_u16(&mut bytes, WIRE_VERSION);
    bytes.push(F64_BITS_CODEC);
    put_u32(&mut bytes, 3);
    put_usize(&mut bytes, 1, "vertex count").unwrap();
    put_optional_f64(&mut bytes, None);
    put_usize(&mut bytes, 0, "bar count").unwrap();
    put_usize(&mut bytes, 1, "space count").unwrap();
    put_usize(&mut bytes, 0, "certificate byte count").unwrap();
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&[0; 32]);
    put_u64(&mut bytes, 0.0f64.to_bits());
    put_u64(&mut bytes, f64::INFINITY.to_bits());
    put_usize(&mut bytes, basis, "basis count").unwrap();
    put_usize(&mut bytes, critical_pairs, "critical-pair count").unwrap();
    bytes
}

fn wire_max_usize() -> usize {
    usize::try_from(u64::MAX).unwrap_or(usize::MAX)
}

#[test]
fn atlas_rejects_space_counts_before_reserving_records() {
    let limits = AtlasDecodeLimits {
        max_basis: usize::MAX,
        max_critical_pairs: usize::MAX,
        ..AtlasDecodeLimits::default()
    };
    let maximum = wire_max_usize();
    for (basis, critical_pairs) in [(1, 0), (0, 1), (maximum, 0), (0, maximum)] {
        let bytes = atlas_with_space_counts(basis, critical_pairs);
        let result = std::panic::catch_unwind(|| {
            AtlasArtifact::decode(&bytes, limits, CertificateLimits::default())
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_err());
    }
}
