use super::*;

use crate::{RipsParams, SparseDistanceMatrix};

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
fn square_class_is_closed_nontrivial_and_stable() {
    let matrix = square();
    let mut first = None;
    for modulus in [2, 3, 5] {
        for threads in [1, 3] {
            let params = RipsParams::new(1)
                .with_modulus(modulus)
                .with_threads(threads);
            let explained = rips_persistence_with_classes_sparse(&matrix, &params).unwrap();
            assert_eq!(explained.class_count(), 1);
            let class = explained.classes().next().unwrap();
            assert_eq!(class.interval.birth, 1.0);
            assert_eq!(class.interval.death, 2.0);
            validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
            if modulus == 2 {
                match first {
                    None => first = Some(class.clone()),
                    Some(ref expected) => assert_eq!(class, expected),
                }
            }
        }
    }
}

#[test]
fn validator_rejects_a_triangle_defect_and_a_coboundary() {
    let triangle =
        SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0)]).unwrap();
    let defect = Cocycle {
        modulus: 2,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    assert!(
        validate_h1_cocycle(&triangle, &defect)
            .unwrap_err()
            .to_string()
            .contains("not closed")
    );

    let path = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 2, 1.0)]).unwrap();
    let coboundary = Cocycle {
        modulus: 2,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    assert!(
        validate_h1_cocycle(&path, &coboundary)
            .unwrap_err()
            .to_string()
            .contains("vertex coboundary")
    );
}

#[test]
fn random_graphs_return_one_valid_class_per_h1_interval() {
    let mut state = 0x17e2_a90c_4b65_d381u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..80 {
        let n = 5 + next() as usize % 9;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 2 {
                    triplets.push((u, v, (next() % 4) as f64));
                }
            }
        }
        let matrix = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus).with_threshold(3.0);
            let explained = rips_persistence_with_classes_sparse(&matrix, &params)
                .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            assert_eq!(
                explained.diagram.in_dim(1).count(),
                explained.class_count(),
                "case {case}, modulus {modulus}"
            );
            for class in explained.classes() {
                validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
            }
        }
    }
}

#[test]
fn collapse_lifts_and_atlases_verify_on_random_graphs() {
    let mut state = 0x8c4f_172d_b365_e902u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..64 {
        let n = 5 + next() as usize % 5;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 3 {
                    triplets.push((u, v, (1 + next() % 4) as f64));
                }
            }
        }
        let matrix = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        for modulus in [2, 3] {
            let baseline = rips_persistence_with_classes_sparse(
                &matrix,
                &RipsParams::new(1).with_modulus(modulus),
            )
            .unwrap();
            let collapsed = crate::collapse::collapse_sparse(&matrix, None).unwrap();
            let mut reduced_params = RipsParams::new(1).with_modulus(modulus);
            reduced_params.threshold = Some(collapsed.certificate.terminal_level());
            let reduced =
                rips_persistence_with_classes_sparse(&collapsed.matrix, &reduced_params).unwrap();
            let explained = lift_h1_classes(&collapsed, reduced)
                .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            assert_eq!(explained.diagram.bars, baseline.diagram.bars);
            for class in explained.classes() {
                validate_h1_cocycle(&matrix, &class.cocycle).unwrap();
            }
            let params = RipsParams::new(1)
                .with_modulus(modulus)
                .with_edge_collapse();
            let artifact =
                crate::AtlasArtifact::build(&matrix, &params, crate::CertificateLimits::default())
                    .unwrap();
            artifact
                .verify(&matrix, crate::CertificateLimits::default())
                .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
        }
    }
}

#[test]
fn duplicate_intervals_form_one_class_space_with_a_canonical_basis() {
    let matrix = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (3, 6, 1.0),
            (3, 5, 2.0),
            (4, 6, 2.0),
        ],
    )
    .unwrap();
    let serial = rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1)).unwrap();
    let parallel =
        rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1).with_threads(4)).unwrap();
    assert_eq!(serial.spaces, parallel.spaces);
    assert_eq!(serial.spaces.len(), 1);
    assert_eq!(serial.spaces[0].basis.len(), 2);
    assert_eq!(serial.spaces[0].basis[0].group_id, serial.spaces[0].id);
    assert_eq!(serial.spaces[0].basis[1].group_id, serial.spaces[0].id);
    assert_ne!(serial.spaces[0].basis[0].id, serial.spaces[0].basis[1].id);
}

#[test]
fn class_space_basis_is_canonical_for_any_seed_order() {
    let matrix = SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (0, 3, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (4, 5, 1.0),
            (4, 7, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
        ],
    )
    .unwrap();
    let cocycle = |terms| Cocycle {
        modulus: 2,
        scale: 1.0,
        terms,
    };
    let first = CocycleTerm {
        u: 2,
        v: 3,
        coefficient: 1,
    };
    let second = CocycleTerm {
        u: 6,
        v: 7,
        coefficient: 1,
    };
    let seeds = [cocycle(vec![second]), cocycle(vec![first, second])];
    let expected = vec![cocycle(vec![first]), cocycle(vec![second])];
    let basis = canonical_space_basis(&matrix, 2, &seeds).unwrap();
    assert_eq!(basis, expected);
    assert_eq!(canonical_space_basis(&matrix, 2, &basis).unwrap(), basis);
}

#[test]
fn every_collapse_schedule_lifts_classes_to_the_original_graph() {
    let matrix = square();
    for schedule in [
        crate::CollapseSchedule::Serial,
        crate::CollapseSchedule::Ordered,
        crate::CollapseSchedule::Rounds,
        crate::CollapseSchedule::Adaptive,
    ] {
        for modulus in [2, 3, 5] {
            let baseline = rips_persistence_with_classes_sparse(
                &matrix,
                &RipsParams::new(1).with_modulus(modulus),
            )
            .unwrap();
            let mut params = RipsParams::new(1)
                .with_modulus(modulus)
                .with_threads(3)
                .with_collapse_schedule(schedule);
            if schedule == crate::CollapseSchedule::Adaptive {
                params.adaptive_collapse = crate::collapse::AdaptiveCollapseParams::new(
                    crate::collapse::CollapseObjective::H1,
                );
            }
            let explained =
                rips_persistence_with_classes_sparse(&matrix, &params).unwrap_or_else(|error| {
                    panic!("schedule {schedule:?}, modulus {modulus}: {error}")
                });
            assert_eq!(explained.diagram.bars, baseline.diagram.bars);
            assert_eq!(explained.class_count(), 1);
            validate_h1_cocycle(&matrix, &explained.classes().next().unwrap().cocycle).unwrap();
            assert_eq!(
                explained.spaces, baseline.spaces,
                "schedule {schedule:?}, modulus {modulus}"
            );
        }
    }
}

#[test]
fn scalar_classes_carry_interval_bound_provenance() {
    let matrix = square();
    let explained =
        rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1).with_modulus(3)).unwrap();
    let class = explained.classes().next().unwrap();
    let provenance = class.provenance().unwrap();
    assert_eq!(provenance.interval(), class.interval);
    assert_eq!(provenance.modulus(), class.cocycle.modulus);
    assert_eq!(provenance.scale().to_bits(), class.cocycle.scale.to_bits());
    assert_eq!(provenance.class_digest(), class.id.as_bytes());
    class.validate_provenance(&matrix).unwrap();

    let changed = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.1),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    let error = class.validate_provenance(&changed).unwrap_err();
    assert!(error.to_string().contains("different active graph"));

    let mut altered = class.clone();
    altered.interval.birth = 0.5;
    let error = altered.validate_provenance(&matrix).unwrap_err();
    assert!(error.to_string().contains("differs from its provenance"));

    let mut altered = class.clone();
    let mut class_digest = *altered.id.as_bytes();
    class_digest[0] ^= 1;
    altered.id = BasisClassId::from_bytes(class_digest);
    let error = altered.validate_provenance(&matrix).unwrap_err();
    assert!(error.to_string().contains("identity differs"));

    let mut altered = class.clone();
    altered.cocycle.terms.push(CocycleTerm {
        u: 1,
        v: 2,
        coefficient: 1,
    });
    let error = altered.validate_provenance(&matrix).unwrap_err();
    assert!(error.to_string().contains("identifier is not canonical"));
}

#[test]
fn provenance_rejects_noncanonical_interval_deaths() {
    let matrix = square();
    let explained =
        rips_persistence_with_classes_sparse(&matrix, &RipsParams::new(1).with_modulus(3)).unwrap();
    let class = explained.classes().next().unwrap();
    for death in [f64::NEG_INFINITY, f64::NAN] {
        let mut altered = class.clone();
        altered.interval.death = death;
        let (source_graph_digest, class_digest, modulus, scale) = {
            let provenance = altered.provenance.as_ref().unwrap();
            (
                *provenance.source_graph_digest(),
                *provenance.class_digest(),
                provenance.modulus(),
                provenance.scale(),
            )
        };
        altered.provenance = Some(PersistentClassProvenance::from_parts(
            source_graph_digest,
            class_digest,
            altered.interval,
            modulus,
            scale,
        ));
        let error = altered.validate_provenance(&matrix).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("representative is outside its interval"),
            "death {death:?}: {error}"
        );
    }
}
