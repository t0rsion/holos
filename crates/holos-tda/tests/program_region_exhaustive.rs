//! Exhaustive small-graph sweeps for result-sensitive persistence programs.

#[path = "program_region_exhaustive/mod.rs"]
mod support;

use holos_tda::{
    CertificateLimits, PersistenceProgram, ReductionCertificate, RipsParams,
    rips_persistence_sparse,
};

use support::{
    complete_edges, diagram_bits_equal, for_each_graph_state, for_each_weighting,
    full_guard_predicate, graph,
};

#[test]
#[ignore = "exhaustive four-vertex region sweep; run with --ignored in release"]
fn every_accepted_four_vertex_weighting_matches_exact_reduction() {
    let mut candidates = 0usize;
    let mut accepted = 0usize;
    for vertex_count in 1..=4 {
        let possible = complete_edges(vertex_count);
        for topology_mask in 0..1usize << possible.len() {
            let topology = possible
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(index, edge)| ((topology_mask >> index) & 1 == 1).then_some(edge))
                .collect::<Vec<_>>();
            for_each_weighting(topology.len(), |initial_weights| {
                let initial = graph(vertex_count, &topology, initial_weights);
                for modulus in [2, 3, 5] {
                    for threshold in [None, Some(0.0), Some(1.0), Some(2.0)] {
                        let mut params = RipsParams::new(1).with_modulus(modulus);
                        params.threshold = threshold;
                        let certificate = ReductionCertificate::build(
                            &initial,
                            &params,
                            CertificateLimits::default(),
                        )
                        .unwrap();
                        let region = certificate
                            .compile_region(&initial, CertificateLimits::default())
                            .unwrap();
                        for_each_weighting(topology.len(), |updated_weights| {
                            candidates += 1;
                            let updated = graph(vertex_count, &topology, updated_weights);
                            let reduced_accepts = region.violations(&updated).is_empty();
                            let complete_accepts =
                                full_guard_predicate(&region, &initial, &updated);
                            assert_eq!(
                                reduced_accepts, complete_accepts,
                                "guard predicates differ for n={vertex_count}, topology={topology:?}, initial={initial_weights:?}, updated={updated_weights:?}, modulus={modulus}, threshold={threshold:?}"
                            );
                            if !reduced_accepts {
                                return;
                            }
                            accepted += 1;
                            let actual = region.evaluate(&updated).unwrap();
                            let expected = rips_persistence_sparse(&updated, &params).unwrap();
                            assert!(
                                diagram_bits_equal(actual.diagram(), &expected),
                                "accepted diagram differs for n={vertex_count}, topology={topology:?}, initial={initial_weights:?}, updated={updated_weights:?}, modulus={modulus}, threshold={threshold:?}"
                            );
                        });
                    }
                }
            });
        }
    }
    assert_eq!(candidates, 12_012_132);
    assert!(accepted > 0);
    println!(
        "format=holos-v010-region-sweep-v1 candidates={candidates} accepted={accepted} exact=yes"
    );
}

#[test]
#[ignore = "exhaustive four-vertex program sweep; run with --ignored in release"]
fn every_four_vertex_program_composes_to_the_monolithic_diagram() {
    let mut checked = 0usize;
    for vertex_count in 1..=4 {
        for_each_graph_state(vertex_count, |input| {
            for modulus in [2, 3, 5] {
                for threshold in [None, Some(0.0), Some(1.0), Some(2.0)] {
                    let mut params = RipsParams::new(1).with_modulus(modulus);
                    params.threshold = threshold;
                    let program =
                        PersistenceProgram::compile(input, &params, CertificateLimits::default())
                            .unwrap();
                    let expected = rips_persistence_sparse(input, &params).unwrap();
                    assert!(diagram_bits_equal(&program.result().diagram, &expected));
                    checked += 1;
                }
            }
        });
    }
    assert_eq!(checked, 49_980);
    println!("format=holos-v010-program-sweep-v1 checked={checked} exact=yes");
}
