use super::topology::diagram_bits_equal;
use super::*;
use crate::{
    CertificateLimits, ClassCorrespondence, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse,
};

fn two_squares() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, 4.5),
        ],
    )
    .unwrap()
}

fn two_cycles_on_zero_edge(separator_weight: f64) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, separator_weight),
            (0, 2, 1.0),
            (2, 3, 2.0),
            (1, 3, 3.0),
            (0, 4, 1.5),
            (4, 5, 2.5),
            (1, 5, 3.5),
        ],
    )
    .unwrap()
}

#[test]
fn diagram_evaluation_recomputes_h0_pairing_after_edge_order_changes() {
    let initial = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 2.0), (2, 3, 3.0), (0, 3, 4.0)],
    )
    .unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 3.0), (1, 2, 2.0), (2, 3, 1.0), (0, 3, 4.0)],
    )
    .unwrap();
    let params = RipsParams::new(1);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();

    let evaluated = program.evaluate_diagram(&updated).unwrap();
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert!(diagram_bits_equal(&evaluated.diagram, &expected));
    assert_eq!(evaluated.work.h0_edges_scanned, updated.num_edges());
}

#[test]
fn program_composes_articulation_blocks_exactly() {
    let graph = two_squares();
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let program =
            PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
        assert_eq!(program.summary().cyclic_atoms, 2);
        assert_eq!(program.summary().articulation_vertices, 1);
        assert!(program.summary().complete_guards >= program.summary().guards);
        assert!(program.summary().guards > 0);
        let expected = rips_persistence_sparse(&graph, &params).unwrap();
        assert!(diagram_bits_equal(&program.result().diagram, &expected));
        assert_eq!(program.result().class_count(), 2);
    }
}

#[test]
fn program_composes_across_a_zero_filtration_edge_separator() {
    let graph = two_cycles_on_zero_edge(0.0);
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let program =
            PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
        assert_eq!(program.summary().cyclic_atoms, 2);
        assert_eq!(program.summary().articulation_vertices, 0);
        assert_eq!(program.summary().zero_simplex_separators, 1);
        assert_eq!(program.summary().widest_separator, 2);
        let expected = rips_persistence_sparse(&graph, &params).unwrap();
        assert!(diagram_bits_equal(&program.result().diagram, &expected));
        assert_eq!(program.result().class_count(), 2);
    }

    let later_separator = two_cycles_on_zero_edge(0.25);
    let unsplit = PersistenceProgram::compile(
        &later_separator,
        &RipsParams::new(1),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(unsplit.summary().cyclic_atoms, 1);
    assert_eq!(unsplit.summary().zero_simplex_separators, 0);

    let mut changing =
        PersistenceProgram::compile(&graph, &RipsParams::new(1), CertificateLimits::default())
            .unwrap();
    let update = changing.advance(&later_separator).unwrap();
    assert_eq!(update.mode, ProgramUpdateMode::Recompiled);
    assert!(
        update
            .events
            .iter()
            .any(|event| event.kind == ProgramEventKind::SeparatorContractChanged)
    );
}

#[test]
fn update_rebuilds_only_the_touched_atom() {
    let graph = two_squares();
    let params = RipsParams::new(1).with_modulus(3);
    let mut program =
        PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 5.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, 4.5),
        ],
    )
    .unwrap();
    let step = program.advance(&updated).unwrap();
    assert_eq!(step.work.atoms_touched, 1);
    assert!(step.work.atoms_rebuilt <= 1);
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert!(diagram_bits_equal(&step.result.diagram, &expected));
}

#[test]
fn ordered_batches_are_atomic_and_checkpoints_restore_exact_state() {
    let initial = two_squares();
    let first = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.1),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, 4.5),
        ],
    )
    .unwrap();
    let second = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.2),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
            (3, 6, 4.5),
        ],
    )
    .unwrap();
    let invalid = SparseDistanceMatrix::from_triplets(6, &[]).unwrap();
    let params = RipsParams::new(1).with_modulus(3);
    let mut program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let checkpoint = program.checkpoint();

    // Force the second, topology-changing step through the compile-time
    // parameter gate after the first step has already succeeded.
    program.params.max_dim = 2;
    assert!(program.advance_batch(&[first.clone(), invalid]).is_err());
    assert!(diagram_bits_equal(
        &program.result().diagram,
        &checkpoint.program().result().diagram
    ));
    assert_eq!(
        program.current_graph().edges().collect::<Vec<_>>(),
        initial.edges().collect::<Vec<_>>()
    );

    program.params.max_dim = 1;
    let updates = program
        .advance_batch(&[first.clone(), second.clone()])
        .unwrap();
    assert_eq!(updates.len(), 2);
    let expected = rips_persistence_sparse(&second, &params).unwrap();
    assert!(diagram_bits_equal(&program.result().diagram, &expected));
    program.restore(&checkpoint);
    assert_eq!(
        program.current_graph().edges().collect::<Vec<_>>(),
        initial.edges().collect::<Vec<_>>()
    );
}

#[test]
fn parallel_branches_preserve_order_and_leave_the_base_unchanged() {
    let initial = two_squares();
    let alternatives: Vec<_> = [1.1, 1.2, 1.3, 1.4]
        .into_iter()
        .map(|weight| {
            SparseDistanceMatrix::from_triplets(
                7,
                &[
                    (0, 1, weight),
                    (1, 2, 2.0),
                    (2, 3, 3.0),
                    (0, 3, 4.0),
                    (3, 4, 1.5),
                    (4, 5, 2.5),
                    (5, 6, 3.5),
                    (3, 6, 4.5),
                ],
            )
            .unwrap()
        })
        .collect();
    let mut params = RipsParams::new(1).with_modulus(5);
    params.threads = 4;
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let branches = program.branch(&alternatives).unwrap();
    assert_eq!(
        branches
            .iter()
            .map(|branch| branch.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    for (branch, graph) in branches.iter().zip(&alternatives) {
        let expected = rips_persistence_sparse(graph, &params).unwrap();
        assert!(diagram_bits_equal(&branch.update.result.diagram, &expected));
        assert!(diagram_bits_equal(
            &branch.program().result().diagram,
            &expected
        ));
    }
    assert_eq!(
        program.current_graph().edges().collect::<Vec<_>>(),
        initial.edges().collect::<Vec<_>>()
    );
}

#[test]
fn unchanged_basis_vectors_receive_exact_continuation() {
    let graph = two_squares();
    let params = RipsParams::new(1);
    let mut program =
        PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.01),
            (1, 2, 2.01),
            (2, 3, 3.01),
            (0, 3, 4.01),
            (3, 4, 1.51),
            (4, 5, 2.51),
            (5, 6, 3.51),
            (3, 6, 4.51),
        ],
    )
    .unwrap();
    let step = program.advance(&updated).unwrap();
    assert!(
        step.continuation
            .iter()
            .all(|record| record.kind == ContinuationKind::Isomorphism)
    );
    assert_eq!(
        step.continuation
            .iter()
            .map(|record| record.transport.len())
            .sum::<usize>(),
        2
    );
    assert!(!step.correspondence.is_empty());
    assert!(
        step.correspondence
            .iter()
            .all(ClassCorrespondence::is_isomorphism)
    );
    assert_eq!(
        step.correspondence
            .iter()
            .map(|record| record.relation_rank)
            .sum::<usize>(),
        2
    );

    let mut state_only =
        PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
    let untracked = state_only
        .advance_with(&updated, CorrespondenceMode::Omit)
        .unwrap();
    assert!(untracked.correspondence.is_empty());
    assert!(diagram_bits_equal(
        &untracked.result.diagram,
        &step.result.diagram
    ));
    assert_eq!(untracked.result.spaces, step.result.spaces);
}

#[test]
fn equal_interval_space_splits_and_merges_without_false_ids() {
    let equal = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.0),
            (4, 5, 2.0),
            (5, 6, 3.0),
            (3, 6, 4.0),
        ],
    )
    .unwrap();
    let split = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.0),
            (4, 5, 2.0),
            (5, 6, 3.0),
            (3, 6, 4.5),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(5);
    let mut program =
        PersistenceProgram::compile(&equal, &params, CertificateLimits::default()).unwrap();
    assert_eq!(program.result().spaces.len(), 1);
    assert_eq!(program.result().spaces[0].basis.len(), 2);
    let separated = program.advance(&split).unwrap();
    assert_eq!(separated.result.spaces.len(), 2);
    assert_eq!(separated.continuation.len(), 1);
    assert_eq!(separated.continuation[0].kind, ContinuationKind::Split);
    let merged = program.advance(&equal).unwrap();
    assert_eq!(merged.result.spaces.len(), 1);
    assert_eq!(merged.continuation.len(), 1);
    assert_eq!(merged.continuation[0].kind, ContinuationKind::Merge);
}

#[test]
fn random_program_updates_match_monolithic_reduction() {
    let mut state = 0x735a_41ce_9b20_d68fu64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..32 {
        let n = 5 + next() as usize % 7;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 3 {
                    triplets.push((u, v, (1 + next() % 12) as f64));
                }
            }
        }
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let mut program =
                PersistenceProgram::compile(&graph, &params, CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            for step in 0..4 {
                let updated_triplets: Vec<_> = triplets
                    .iter()
                    .map(|&(u, v, value)| {
                        let delta = (next() % 9) as f64 * 0.1 * (step + 1) as f64;
                        (u, v, value + delta)
                    })
                    .collect();
                let updated = SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
                let actual = program.advance(&updated).unwrap_or_else(|error| {
                    panic!("case {case}, modulus {modulus}, step {step}: {error}")
                });
                let expected = rips_persistence_sparse(&updated, &params).unwrap();
                assert!(
                    diagram_bits_equal(&actual.result.diagram, &expected),
                    "case {case}, modulus {modulus}, step {step}"
                );
            }
        }
    }
}

#[test]
fn diagram_state_reuses_regions_and_materializes_factorized_classes() {
    let initial = two_squares();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.01),
            (1, 2, 2.01),
            (2, 3, 3.01),
            (0, 3, 4.01),
            (3, 4, 1.51),
            (4, 5, 2.51),
            (5, 6, 3.51),
            (3, 6, 4.51),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(5);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let mut state = program.into_diagram_state();

    let step = state.advance(&updated).unwrap();
    assert_eq!(step.mode, ProgramDiagramUpdateMode::Reused);
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert!(diagram_bits_equal(&step.diagram, &expected));

    let materialized = state.materialize().unwrap().clone();
    let direct =
        PersistenceProgram::compile(&updated, &params, CertificateLimits::default()).unwrap();
    assert_eq!(materialized.spaces, direct.result().spaces);
    assert!(diagram_bits_equal(
        &materialized.diagram,
        &direct.result().diagram
    ));
    assert!(diagram_bits_equal(state.diagram(), &materialized.diagram));
}

#[test]
fn diagram_state_noop_keeps_the_rich_program_clean() {
    let initial = two_squares();
    let params = RipsParams::new(1);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let mut state = program.into_diagram_state();

    let step = state.advance(&initial).unwrap();
    assert_eq!(step.mode, ProgramDiagramUpdateMode::Reused);
    assert_eq!(step.work.atoms_touched, 0);
    assert!(!state.dirty);
    assert!(diagram_bits_equal(&step.diagram, state.diagram()));
}

#[test]
fn diagram_state_topology_fallback_is_exact() {
    let initial = two_squares();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(3);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let mut state = program.into_diagram_state();

    let step = state.advance(&updated).unwrap();
    assert_eq!(step.mode, ProgramDiagramUpdateMode::Recompiled);
    assert!(
        step.events
            .iter()
            .any(|event| event.kind == ProgramEventKind::EdgeSetChanged)
    );
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert!(diagram_bits_equal(&step.diagram, &expected));
    let materialized = state.materialize().unwrap().clone();
    let direct =
        PersistenceProgram::compile(&updated, &params, CertificateLimits::default()).unwrap();
    assert_eq!(materialized.spaces, direct.result().spaces);
}

#[test]
fn diagram_state_guard_failure_uses_the_exact_fallback() {
    let initial = two_squares();
    let params = RipsParams::new(1).with_modulus(3);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let mut state = program.into_diagram_state();
    let make_update = |weight| {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, weight),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap()
    };
    let updated = [5.0, 7.0, 9.0, 12.0]
        .into_iter()
        .map(make_update)
        .find(|candidate| {
            state.program.states().iter().any(|atom| {
                let local =
                    super::composition::local_matrix(&atom.vertices, &atom.edges, candidate)
                        .unwrap();
                atom.region
                    .violations(&local)
                    .iter()
                    .any(|violation| violation.kind() == crate::RegionViolationKind::GuardFailed)
            })
        })
        .expect("the guard-failure candidates must leave the initial region");

    let step = state.advance(&updated).unwrap();
    assert_eq!(step.mode, ProgramDiagramUpdateMode::Recompiled);
    assert!(
        step.events
            .iter()
            .any(|event| event.kind == ProgramEventKind::GuardFailed)
    );
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert!(diagram_bits_equal(&step.diagram, &expected));
}

#[test]
fn diagram_state_failed_fallback_preserves_current_snapshot() {
    let initial = two_squares();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (2, 3, 3.0),
            (0, 3, 4.0),
            (3, 4, 1.5),
            (4, 5, 2.5),
            (5, 6, 3.5),
        ],
    )
    .unwrap();
    let program =
        PersistenceProgram::compile(&initial, &RipsParams::new(1), CertificateLimits::default())
            .unwrap();
    let mut state = program.into_diagram_state();
    let before = state.diagram().clone();
    let before_graph: Vec<_> = state.graph.edges().collect();
    state.program.params.max_dim = 2;

    assert!(state.advance(&updated).is_err());
    assert!(diagram_bits_equal(state.diagram(), &before));
    assert_eq!(state.graph.edges().collect::<Vec<_>>(), before_graph);
}

#[test]
fn diagram_state_rejects_a_mismatching_materialization_fallback() {
    let initial = two_squares();
    let updated = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.01),
            (1, 2, 2.01),
            (2, 3, 3.01),
            (0, 3, 4.01),
            (3, 4, 1.51),
            (4, 5, 2.51),
            (5, 6, 3.51),
            (3, 6, 4.51),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(5);
    let program =
        PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
    let mut state = program.into_diagram_state();
    state.advance(&updated).unwrap();
    let before_program = state.program.clone();
    let before_graph: Vec<_> = state.graph.edges().collect();
    state.diagram.bars[0].death += 1.0;
    let before_diagram = state.diagram.clone();

    assert!(state.materialize().is_err());
    assert!(diagram_bits_equal(state.diagram(), &before_diagram));
    assert_eq!(state.graph.edges().collect::<Vec<_>>(), before_graph);
    assert!(diagram_bits_equal(
        &state.program.result().diagram,
        &before_program.result().diagram
    ));
    assert_eq!(
        state.program.result().spaces,
        before_program.result().spaces
    );
    assert!(state.dirty);
}
