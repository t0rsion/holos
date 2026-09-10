use sha2::{Digest, Sha256};

use super::model::{CircularProofLimits, MAGIC};
use super::{is_circular_coordinate, verify_circular_coordinate};

#[test]
fn rejects_short_and_arbitrary_inputs_without_panicking() {
    for bytes in [&[][..], &[0u8; 31], &[0u8; 32], b"HOLOSCC\0"] {
        assert!(verify_circular_coordinate(bytes, CircularProofLimits::default()).is_err());
    }
}

#[test]
fn rejects_a_digest_valid_truncated_payload() {
    let mut bytes = MAGIC.to_vec();
    let mut hash = Sha256::new();
    hash.update(b"holos-circular-coordinate-v1");
    hash.update(&bytes);
    bytes.extend_from_slice(&hash.finalize());
    assert!(is_circular_coordinate(&bytes));
    assert!(verify_circular_coordinate(&bytes, CircularProofLimits::default()).is_err());
}
