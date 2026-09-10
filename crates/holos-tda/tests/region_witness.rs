#[path = "fixtures/region_witness.rs"]
mod fixture;

use holos_tda::{
    CertificateLimits, PersistenceAtlas, ReductionCertificate, TopologyEventKind,
    rips_persistence_sparse,
};

#[test]
fn fixed_witness_crosses_a_complete_order_boundary() {
    let initial = fixture::initial_graph();
    let updated = fixture::updated_graph();
    let params = fixture::params();
    let certificate =
        ReductionCertificate::build(&initial, &params, CertificateLimits::default()).unwrap();
    let region = certificate
        .compile_region(&initial, CertificateLimits::default())
        .unwrap();
    let atlas = PersistenceAtlas::build(&initial, &params).unwrap();

    let events = atlas.events(&updated);
    assert_eq!(events.len(), 1);
    assert!(matches!(events.as_slice(), [event]
        if event.kind == TopologyEventKind::OrderSwap
            && event.first.map(|edge| [edge.u, edge.v]) == Some(fixture::REVERSED_FIRST)
            && event.second.map(|edge| [edge.u, edge.v]) == Some(fixture::REVERSED_SECOND)));
    assert!(region.violations(&initial).is_empty());
    assert!(region.violations(&updated).is_empty());
    assert!(fixture::guards_hold(&initial, region.complete_guards()));
    assert!(fixture::guards_hold(&updated, region.complete_guards()));
    assert!(fixture::guards_hold(&updated, region.guards()));
    assert!(!region.complete_guards().is_empty());
    assert!(!region.guards().is_empty());
    assert!(region.guards().len() <= region.complete_guards().len());
    assert!(region.guards().len() < initial.edges().count() * (initial.edges().count() - 1) / 2);
    assert!(region.complete_guards().iter().all(|guard| {
        !(guard.earlier().vertices() == fixture::REVERSED_FIRST
            && guard.later().vertices() == fixture::REVERSED_SECOND)
    }));
}

#[test]
fn fixed_witness_reconstructs_exact_diagrams_and_h1_pairs() {
    let initial = fixture::initial_graph();
    let updated = fixture::updated_graph();
    let params = fixture::params();
    let certificate =
        ReductionCertificate::build(&initial, &params, CertificateLimits::default()).unwrap();
    let region = certificate
        .compile_region(&initial, CertificateLimits::default())
        .unwrap();

    let initial_evaluation = region.evaluate(&initial).unwrap();
    let updated_evaluation = region.evaluate(&updated).unwrap();
    let initial_exact = rips_persistence_sparse(&initial, &params).unwrap();
    let updated_exact = rips_persistence_sparse(&updated, &params).unwrap();

    assert_eq!(
        initial_evaluation.diagram().bars,
        fixture::expected_initial_bars()
    );
    assert_eq!(
        updated_evaluation.diagram().bars,
        fixture::expected_updated_bars()
    );
    assert!(fixture::diagram_bits_equal(
        initial_evaluation.diagram(),
        &initial_exact
    ));
    assert!(fixture::diagram_bits_equal(
        updated_evaluation.diagram(),
        &updated_exact
    ));

    let initial_pairs = initial_evaluation.h1_critical_pairs();
    let updated_pairs = updated_evaluation.h1_critical_pairs();
    assert_eq!(initial_pairs.len(), 2);
    assert_eq!(updated_pairs.len(), 2);
    assert_eq!(initial_pairs[0].1.birth.vertices, fixture::REVERSED_FIRST);
    assert_eq!(initial_pairs[0].1.birth.value, 4.0);
    assert!(initial_pairs[0].1.death.is_none());
    assert_eq!(initial_pairs[1].1.birth.vertices, [0, 6]);
    assert_eq!(initial_pairs[1].1.birth.value, 8.0);
    assert!(initial_pairs[1].1.death.is_none());
    assert_eq!(updated_pairs[0].1.birth.vertices, fixture::REVERSED_FIRST);
    assert_eq!(updated_pairs[0].1.birth.value, 5.0);
    assert!(updated_pairs[0].1.death.is_none());
    assert_eq!(updated_pairs[1].1.birth.vertices, [0, 6]);
    assert_eq!(updated_pairs[1].1.birth.value, 8.0);
    assert!(updated_pairs[1].1.death.is_none());
}
