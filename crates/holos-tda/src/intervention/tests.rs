use super::{
    InterventionArtifact, InterventionBudget, InterventionDecodeLimits, InterventionStatus,
};
use crate::certificate::CertificateLimits;
use crate::{PersistenceProgram, SparseDistanceMatrix};
use proptest::prelude::*;

fn square() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 3.0),
            (1, 3, 3.0),
        ],
    )
    .unwrap()
}

#[test]
fn finite_class_intervention_is_feasible_and_independently_checked() {
    let graph = square();
    let program = PersistenceProgram::compile(
        &graph,
        &crate::RipsParams::new(1).with_modulus(3),
        CertificateLimits::default(),
    )
    .unwrap();
    let target = program.result().spaces[0].id;
    let intervention = program
        .kill_h1_before(target, 2.5, InterventionBudget::default())
        .unwrap();
    assert!(matches!(
        intervention.status,
        InterventionStatus::Optimal | InterventionStatus::BoundedGap
    ));
    assert!(intervention.upper_bound.is_some());
    assert!(
        intervention
            .result
            .as_ref()
            .unwrap()
            .spaces
            .iter()
            .all(|space| space.interval.death <= 2.5)
    );
    let artifact = intervention.artifact.unwrap();
    let bytes = artifact.encode().unwrap();
    let decoded = InterventionArtifact::decode(
        &bytes,
        InterventionDecodeLimits::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    decoded.verify(CertificateLimits::default()).unwrap();
}

#[test]
fn zero_budget_and_essential_classes_are_honest() {
    let graph = square();
    let program = PersistenceProgram::compile(
        &graph,
        &crate::RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    let target = program.result().spaces[0].id;
    let limited = program
        .kill_h1_before(target, 2.5, InterventionBudget::new(0))
        .unwrap();
    assert_eq!(limited.status, InterventionStatus::BudgetLimited);
    assert!(limited.artifact.is_none());

    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let essential = PersistenceProgram::compile(
        &cycle,
        &crate::RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    assert!(
        essential
            .kill_h1_before(
                essential.result().spaces[0].id,
                2.0,
                InterventionBudget::default()
            )
            .is_err()
    );
}

#[test]
fn mutations_and_truncation_are_rejected() {
    let graph = square();
    let program = PersistenceProgram::compile(
        &graph,
        &crate::RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    let intervention = program
        .kill_h1_before(
            program.result().spaces[0].id,
            2.5,
            InterventionBudget::default(),
        )
        .unwrap();
    let artifact = intervention.artifact.unwrap();
    let bytes = artifact.encode().unwrap();
    assert!((0..bytes.len()).all(|end| {
        InterventionArtifact::decode(
            &bytes[..end],
            InterventionDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .is_err()
    }));
    let mut changed = artifact.clone();
    changed.upper_bound += 1.0;
    assert!(changed.verify(CertificateLimits::default()).is_err());
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = InterventionArtifact::decode(
            &bytes,
            InterventionDecodeLimits::default(),
            CertificateLimits::default(),
        );
    }
}
