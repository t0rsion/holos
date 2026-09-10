use super::codec::diagram_bits_equal;
use super::*;
use crate::{
    CertificateLimits, ContinuationKind, ProgramEventKind, ProgramUpdateMode, RipsParams,
    SparseDistanceMatrix,
};
use proptest::prelude::*;

fn graph(first: f64, second: f64) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, first),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, second),
        ],
    )
    .unwrap()
}

#[test]
fn trace_round_trips_reuse_repair_and_continuation() {
    let initial = graph(4.0, 4.0);
    let updates = [graph(4.01, 4.01), graph(5.0, 4.01), graph(4.0, 4.0)];
    let trace = ProgramTraceArtifact::build(
        &initial,
        &updates,
        &RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    assert!(trace.steps.iter().any(|step| {
        step.continuation
            .iter()
            .any(|record| record.kind == ContinuationKind::Split)
    }));
    let bytes = trace.encode().unwrap();
    let decoded = ProgramTraceArtifact::decode(
        &bytes,
        ProgramTraceDecodeLimits::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    let verified = decoded.verify(CertificateLimits::default()).unwrap();
    assert_eq!(verified.steps.len(), updates.len());
    assert!(diagram_bits_equal(
        &verified.steps.last().unwrap().diagram,
        &verified.final_program.result().diagram
    ));
}

#[test]
fn trace_replays_a_retained_reduction_suffix() {
    let initial = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 2.0),
            (0, 3, 3.0),
            (1, 2, 4.0),
            (1, 3, 5.0),
            (2, 3, 6.0),
        ],
    )
    .unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 2.0),
            (0, 3, 3.0),
            (1, 2, 4.0),
            (1, 3, 6.5),
            (2, 3, 6.0),
        ],
    )
    .unwrap();
    let trace = ProgramTraceArtifact::build(
        &initial,
        &[updated],
        &RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(trace.steps[0].mode, ProgramUpdateMode::Repaired);
    assert_eq!(trace.steps[0].work.atoms_repaired, 1);
    assert!(trace.steps[0].work.reduction_columns_reused > 0);
    trace.verify(CertificateLimits::default()).unwrap();
}

#[test]
fn class_space_rebuild_retains_the_reduction() {
    let edges = [
        (0, 1, 2.0),
        (0, 2, 1.0),
        (0, 3, 1.0),
        (0, 4, 1.0),
        (1, 2, 1.0),
        (1, 3, 1.0),
        (1, 4, 1.0),
    ];
    let initial = SparseDistanceMatrix::from_triplets(5, &edges).unwrap();
    let mut updated_edges = edges;
    updated_edges[1].2 = 1.1;
    let updated = SparseDistanceMatrix::from_triplets(5, &updated_edges).unwrap();
    let trace = ProgramTraceArtifact::build(
        &initial,
        &[updated],
        &RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(trace.initial_program().atoms().len(), 1);
    assert_eq!(trace.initial_program().atoms()[0].atlas().spaces().len(), 1);
    assert_eq!(
        trace.initial_program().atoms()[0].atlas().spaces()[0]
            .basis
            .len(),
        2
    );
    let step = &trace.steps()[0];
    assert_eq!(step.mode(), ProgramUpdateMode::Repaired);
    assert_eq!(step.work().atoms_repaired, 0);
    assert_eq!(step.work().atoms_rebuilt, 1);
    assert_eq!(step.work().reduction_columns_reduced, 0);
    assert!(step.work().reduction_columns_reused > 0);
    assert_eq!(step.events().len(), 1);
    assert_eq!(step.events()[0].kind, ProgramEventKind::AtomRebuilt);
    trace.verify(CertificateLimits::default()).unwrap();
}

#[test]
fn changed_work_truncation_and_trailing_bytes_are_rejected() {
    let initial = graph(4.0, 4.0);
    let updates = [graph(4.01, 4.01), graph(5.0, 4.01)];
    let trace = ProgramTraceArtifact::build(
        &initial,
        &updates,
        &RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    let bytes = trace.encode().unwrap();
    assert!((0..bytes.len()).all(|end| {
        ProgramTraceArtifact::decode(
            &bytes[..end],
            ProgramTraceDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    }));
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(
        ProgramTraceArtifact::decode(
            &trailing,
            ProgramTraceDecodeLimits::default(),
            CertificateLimits::default()
        )
        .is_err()
    );
    let mut changed = trace.clone();
    changed.steps[0].work.edges_checked += 1;
    assert!(changed.verify(CertificateLimits::default()).is_err());
}

#[test]
fn trace_step_count_checks_remaining_bytes_before_allocation() {
    let initial = graph(4.0, 4.0);
    let trace = ProgramTraceArtifact::build(
        &initial,
        &[],
        &RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    let mut bytes = trace.encode().unwrap();
    bytes[11..19].copy_from_slice(&10_000_u64.to_be_bytes());
    let limits = ProgramTraceDecodeLimits {
        max_steps: 10_000,
        ..ProgramTraceDecodeLimits::default()
    };
    let error =
        ProgramTraceArtifact::decode(&bytes, limits, CertificateLimits::default()).unwrap_err();
    assert!(error.message().contains("trace step records"));
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = ProgramTraceArtifact::decode(
            &bytes,
            ProgramTraceDecodeLimits::default(),
            CertificateLimits::default(),
        );
    }
}
