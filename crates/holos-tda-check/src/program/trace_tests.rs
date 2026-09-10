use super::{ProgramTraceProofLimits, is_program_trace, verify_program_trace};

const PRODUCER_TRACE: &str = include_str!("trace_fixture.hex");
const CLASS_SPACE_REBUILD_TRACE: &str = include_str!("trace_class_space_rebuild.hex");

fn producer_trace_bytes() -> Vec<u8> {
    let hex = PRODUCER_TRACE.trim();
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}

fn class_space_rebuild_trace_bytes() -> Vec<u8> {
    let hex = CLASS_SPACE_REBUILD_TRACE.trim();
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}

#[test]
fn producer_trace_covers_reuse_repair_and_rebuild() {
    let bytes = producer_trace_bytes();
    assert!(is_program_trace(&bytes));
    let checked = verify_program_trace(&bytes, ProgramTraceProofLimits::default()).unwrap();
    assert_eq!(checked.modulus, 3);
    assert_eq!(checked.vertices, 7);
    assert_eq!(checked.edges, 8);
    assert_eq!(checked.steps, 3);
    assert_eq!(checked.reused_steps, 1);
    assert_eq!(checked.repaired_steps, 1);
    assert_eq!(checked.recompiled_steps, 1);
    assert_eq!(checked.bars, 8);
}

#[test]
fn class_space_rebuild_with_retained_reduction_is_independently_checked() {
    let bytes = class_space_rebuild_trace_bytes();
    let checked = verify_program_trace(&bytes, ProgramTraceProofLimits::default()).unwrap();
    assert_eq!(checked.steps, 1);
    assert_eq!(checked.repaired_steps, 1);
    assert_eq!(checked.recompiled_steps, 0);
}

#[test]
fn trace_mutations_are_rejected() {
    let bytes = producer_trace_bytes();
    for offset in (0..bytes.len()).step_by(17) {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x80;
        assert!(
            verify_program_trace(&changed, ProgramTraceProofLimits::default()).is_err(),
            "mutation at byte {offset} was accepted"
        );
    }
}

#[test]
#[ignore = "exhaustive single-bit mutation sweep"]
fn every_trace_bit_mutation_is_rejected() {
    let bytes = producer_trace_bytes();
    for offset in 0..bytes.len() {
        for bit in 0..8 {
            let mask = 1 << bit;
            let mut changed = bytes.clone();
            changed[offset] ^= mask;
            assert!(
                verify_program_trace(&changed, ProgramTraceProofLimits::default()).is_err(),
                "mutation {mask:#04x} at byte {offset} was accepted"
            );
        }
    }
}

#[test]
fn trace_truncations_are_rejected() {
    let bytes = producer_trace_bytes();
    for end in 0..bytes.len() {
        assert!(
            verify_program_trace(&bytes[..end], ProgramTraceProofLimits::default()).is_err(),
            "truncation at byte {end} was accepted"
        );
    }
}

#[test]
fn arbitrary_trace_bytes_do_not_panic() {
    for length in 0..4096 {
        let bytes: Vec<u8> = (0..length)
            .map(|index| (index as u8).wrapping_mul(73).wrapping_add(19))
            .collect();
        let _ = verify_program_trace(&bytes, ProgramTraceProofLimits::default());
    }
}
