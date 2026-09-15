use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::cohomology::Edge;

use super::linear::{
    check_field_triangle_closure, check_integer_triangle_closure, check_reduction,
    integral_divisibility,
};
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

#[test]
fn limits_reject_a_coefficient_bound_outside_signed_wire_range() {
    let mut limits = CircularProofLimits {
        max_integral_coefficient: i64::MAX as u64,
        ..CircularProofLimits::default()
    };
    assert!(limits.validate().is_ok());
    limits.max_integral_coefficient = i64::MAX as u64 + 1;
    assert!(limits.validate().is_err());
}

#[test]
fn integer_checks_report_reversed_minimum_without_panicking() {
    let edges = [Edge { u: 0, v: 2 }, Edge { u: 1, v: 2 }];
    let coefficients = BTreeMap::from([(Edge { u: 1, v: 2 }, i64::MIN)]);
    let error = integral_divisibility(3, &edges, &coefficients).unwrap_err();
    assert!(error.message().contains("cannot be reversed"));
}

#[test]
fn integer_triangle_checks_report_sum_overflow_without_panicking() {
    let edges = [
        Edge { u: 0, v: 1 },
        Edge { u: 0, v: 2 },
        Edge { u: 1, v: 2 },
    ];
    let coefficients = BTreeMap::from([
        (Edge { u: 0, v: 1 }, i64::MAX),
        (Edge { u: 0, v: 2 }, i64::MAX),
        (Edge { u: 1, v: 2 }, i64::MAX),
    ]);
    let error = check_integer_triangle_closure(3, &edges, &coefficients).unwrap_err();
    assert!(error.message().contains("boundary overflows"));
}

#[test]
fn shared_linear_checks_reject_zero_modulus_before_remainder() {
    let edges = [
        Edge { u: 0, v: 1 },
        Edge { u: 0, v: 2 },
        Edge { u: 1, v: 2 },
    ];
    let field_error = check_field_triangle_closure(3, &edges, &[], 0).unwrap_err();
    assert!(field_error.message().contains("modulus is invalid"));
    let integral_error = check_reduction(&edges, &[], &BTreeMap::new(), 1, 0).unwrap_err();
    assert!(integral_error.message().contains("modulus is invalid"));
}
