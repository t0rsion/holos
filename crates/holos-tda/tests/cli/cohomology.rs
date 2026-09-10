use super::*;

#[test]
fn dimension_generic_cohomology_cli_relates_h2() {
    let graph = octahedral_sphere_file("cohomology_sphere.spr");
    let out = run(&[
        "cohomology",
        graph.path().to_str().unwrap(),
        graph.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--at",
        "1",
        "--homology-dim",
        "2",
        "--modulus",
        "5",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("H2 at 1 over Z/5 has rank 1"), "{text}");
    assert!(text.contains("relation rank 1"), "{text}");
    assert!(text.contains("isomorphism true"), "{text}");
}

#[test]
fn kinetic_cli_reports_exact_h2_change() {
    let mut text = String::new();
    for u in 0..6 {
        for v in u + 1..6 {
            if u / 2 != v / 2 {
                text.push_str(&format!("{u} {v} 0 0\n"));
            }
        }
    }
    text.push_str("0 1 2 -2\n");
    let trajectory = TempFile::new("sphere.kin", &text);
    let artifact = TempFile::new("sphere.hzz", "");
    let out = run(&[
        "kinetic",
        trajectory.path().to_str().unwrap(),
        "--vertices",
        "6",
        "--start",
        "0",
        "--end",
        "1",
        "--at",
        "1",
        "--homology-dim",
        "2",
        "--modulus",
        "3",
        "--zigzag",
        artifact.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("threshold (0, 1)"), "{text}");
    assert!(text.contains("rank 1 to 0, relation rank 0"), "{text}");
    assert!(text.contains("kinetic zigzag with"), "{text}");
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_kinetic_zigzag(&bytes));
    let checked = verify_kinetic_zigzag(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.dimension, 2);
}

#[test]
fn cohomology_intervention_cli_writes_an_independent_artifact() {
    let graph = octahedral_sphere_file("intervention_sphere.spr");
    let artifact = TempFile::new("intervention.hci", "");
    let out = run(&[
        "intervene-cohomology",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--at",
        "1",
        "--homology-dim",
        "2",
        "--target",
        "0",
        "--candidate",
        "0",
        "1",
        "1",
        "--max-edits",
        "1",
        "--modulus",
        "5",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_cohomology_intervention(&bytes));
    let checked = verify_cohomology_intervention(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.dimension, 2);
    assert_eq!(checked.edits, 1);
    assert_eq!(checked.lower_bound_cost, Some(1));
    assert_eq!(checked.upper_bound_cost, Some(1));
}

#[test]
fn link_plan_cli_certifies_one_weighted_plan_across_scenarios() {
    let graph = TempFile::new(
        "link_scenarios.spr",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n4 5 1\n5 6 1\n6 7 1\n4 7 1\n",
    );
    let artifact = TempFile::new("link_plan.hci", "");
    let out = run(&[
        "plan-links",
        artifact.path().to_str().unwrap(),
        "--vertices",
        "8",
        "--scenario",
        graph.path().to_str().unwrap(),
        "--target",
        "0",
        "--scenario",
        graph.path().to_str().unwrap(),
        "--target",
        "1",
        "--at",
        "1",
        "--homology-dim",
        "1",
        "--candidate",
        "0",
        "2",
        "4",
        "--candidate",
        "4",
        "6",
        "7",
        "--max-edits",
        "2",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_cohomology_intervention(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.scenarios, 2);
    assert_eq!(checked.edits, 2);
    assert_eq!(checked.total_cost, Some(11));
    assert_eq!(checked.lower_bound_cost, Some(11));
    assert_eq!(checked.upper_bound_cost, Some(11));
    assert_eq!(checked.before_ranks, vec![2, 2]);
    assert_eq!(checked.after_ranks, vec![0, 0]);
}

#[test]
fn synthesis_cli_writes_a_proof_checked_without_search_replay() {
    let graph = TempFile::new(
        "synthesis_states.spr",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n4 5 1\n5 6 1\n6 7 1\n4 7 1\n",
    );
    let artifact = TempFile::new("synthesis.hsyn", "");
    let out = run(&[
        "synthesize",
        artifact.path().to_str().unwrap(),
        "--state",
        graph.path().to_str().unwrap(),
        "--state",
        graph.path().to_str().unwrap(),
        "--vertices",
        "8",
        "--at",
        "1",
        "--homology-dim",
        "1",
        "--max-rank",
        "0",
        "--candidate",
        "0",
        "2",
        "4",
        "--candidate",
        "4",
        "6",
        "7",
        "--max-edits",
        "2",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("proof topology checks"));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_synthesis(&bytes));
    let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.states, 2);
    assert_eq!(checked.selected, 2);
    assert_eq!(checked.total_cost, Some(11));
    assert!(checked.proof_topology_checks < checked.producer_oracle_calls);
}

#[test]
fn kinetic_synthesis_cli_covers_the_complete_event_schedule() {
    let trajectory = TempFile::new(
        "synthesis.kin",
        "0 1 1 0\n1 2 1 0\n2 3 1 0\n0 3 1 0\n0 2 2 -1\n",
    );
    let artifact = TempFile::new("kinetic_synthesis.hsyn", "");
    let out = run(&[
        "synthesize-kinetic",
        trajectory.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--vertices",
        "4",
        "--start",
        "0",
        "--end",
        "1.5",
        "--at",
        "1",
        "--candidate",
        "1",
        "3",
        "2",
        "--max-edits",
        "1",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("all-time Rips rank plan"));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(
        checked.status,
        holos_tda_check::VerifiedSynthesisStatus::Optimal
    );
    assert_eq!(checked.total_cost, Some(2));
}
