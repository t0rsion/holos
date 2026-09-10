use super::*;

#[test]
fn coverage_cli_certifies_failures_and_warns_about_geometry() {
    let graph = TempFile::new(
        "coverage.spr",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n0 4 1\n1 4 1\n2 4 1\n3 4 1\n0 5 1\n1 5 1\n2 5 1\n3 5 1\n",
    );
    let artifact = TempFile::new("coverage.hcov", "");
    let out = run(&[
        "cover",
        artifact.path().to_str().unwrap(),
        "--state",
        graph.path().to_str().unwrap(),
        "--vertices",
        "6",
        "--broadcast-radius",
        "1",
        "--sensing-radius",
        "1",
        "--fence",
        "0,1,2,3",
        "--failable",
        "4,5",
        "--failure-budget",
        "1",
        "--candidate",
        "4",
        "2",
        "all",
        "--candidate",
        "5",
        "3",
        "all",
        "--max-activations",
        "2",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("relative coverage"));
    assert!(stderr(&out).contains("physical coverage requires"));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_coverage(&bytes));
    let checked = verify_coverage(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.selected, 2);
    assert_eq!(checked.total_cost, Some(5));
    assert_eq!(checked.failure_budget, 1);
}

#[test]
fn collapse_portfolio_cli_selects_and_encodes_every_schedule() {
    let graph = TempFile::new(
        "portfolio.spr",
        "0 1 1\n0 2 1\n0 3 1\n1 2 1\n1 3 1\n2 3 1\n",
    );
    let artifact = TempFile::new("portfolio.hpor", "");
    let out = run(&[
        "collapse-portfolio",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--threads",
        "2",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("selected candidate"));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let decoded = CollapsePortfolioArtifact::decode(
        &bytes,
        CollapsePortfolioLimits::default(),
        CollapsePortfolioDecodeLimits::default(),
    )
    .unwrap();
    let input = holos_tda::io::read_sparse_matrix(graph.path(), 1).unwrap();
    decoded
        .verify_sparse(&input, None, CollapsePortfolioLimits::default())
        .unwrap();
    assert_eq!(decoded.entries().len(), 3);
}

#[test]
fn coverage_cli_binds_finite_states_to_planar_coordinates() {
    let graph = TempFile::new(
        "coverage_geometry.spr",
        "0 1 2\n1 2 2\n2 3 2\n0 3 2\n0 4 1.4142135623730951\n1 4 1.4142135623730951\n2 4 1.4142135623730951\n3 4 1.4142135623730951\n",
    );
    let coordinates = TempFile::new("coverage_geometry.pts", "0 0\n2 0\n2 2\n0 2\n1 1\n");
    let artifact = TempFile::new("coverage_geometry.hgeo", "");
    let out = run(&[
        "cover",
        artifact.path().to_str().unwrap(),
        "--state",
        graph.path().to_str().unwrap(),
        "--coordinates",
        coordinates.path().to_str().unwrap(),
        "--vertices",
        "5",
        "--broadcast-radius",
        "2",
        "--sensing-radius",
        "2",
        "--fence",
        "0,1,2,3",
        "--candidate",
        "4",
        "1",
        "all",
        "--max-activations",
        "1",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!stderr(&out).contains("physical coverage requires"));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_geometry_bound_coverage(&bytes));
    let checked = verify_geometry_bound_coverage(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.vertices, 5);
    assert_eq!(checked.pair_checks, 10);
    assert_eq!(checked.coverage.total_cost, Some(1));
}

#[test]
fn circular_cli_accepts_ripser_terms_and_writes_a_checked_artifact() {
    let graph = TempFile::new(
        "circular.spr",
        "0 1 1\n1 2 1\n2 3 1\n3 4 1\n4 5 1\n5 6 1\n6 7 1\n0 7 1\n",
    );
    let cocycle = TempFile::new("circular.cocycle", "0 1 1\n");
    let artifact = TempFile::new("circular.hcc", "");
    let phases = TempFile::new("circular.phase", "");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        cocycle.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--at",
        "1",
        "--format",
        "sparse",
        "--phases",
        phases.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_circular_coordinate(&bytes));
    let checked = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.coordinates, 1);
    assert_eq!(
        std::fs::read_to_string(phases.path())
            .unwrap()
            .lines()
            .count(),
        8
    );
}

#[test]
fn circular_cli_consumes_a_graph_bound_persistent_class() {
    let graph = TempFile::new(
        "circular_class.spr",
        "0 1 1\n1 2 1\n2 3 1\n3 4 1\n4 5 1\n5 6 1\n6 7 1\n0 7 1\n",
    );
    let classes = TempFile::new("circular_classes.json", "");
    let artifact = TempFile::new("circular_class.hcc", "");
    let phases = TempFile::new("circular_class.phase", "");
    let computed = run(&[
        graph.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "1",
        "--modulus",
        "47",
        "--representatives",
        classes.path().to_str().unwrap(),
    ]);
    assert!(computed.status.success(), "stderr: {}", stderr(&computed));

    let circular = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        classes.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--class",
        "0",
        "0",
        "--format",
        "sparse",
        "--phases",
        phases.path().to_str().unwrap(),
    ]);
    assert!(circular.status.success(), "stderr: {}", stderr(&circular));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_circular_coordinate(&bytes));
    verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();

    let changed = TempFile::new(
        "circular_class_changed.spr",
        "0 1 0.5\n1 2 1\n2 3 1\n3 4 1\n4 5 1\n5 6 1\n6 7 1\n0 7 1\n",
    );
    let rejected = run(&[
        "circular",
        changed.path().to_str().unwrap(),
        classes.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--class",
        "0",
        "0",
        "--format",
        "sparse",
    ]);
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("different active graph"),
        "{}",
        stderr(&rejected)
    );
}

#[test]
fn affine_coverage_cli_binds_the_complete_schedule() {
    let trajectory = TempFile::new(
        "coverage.kin",
        "0 1 1 0\n1 2 1 0\n2 3 1 0\n0 3 1 0\n0 4 1 0\n1 4 1 0\n2 4 1 0\n3 4 1 0\n",
    );
    let artifact = TempFile::new("coverage_affine.hcov", "");
    let out = run(&[
        "cover-affine",
        trajectory.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--vertices",
        "5",
        "--start",
        "0",
        "--end",
        "1",
        "--broadcast-radius",
        "1",
        "--sensing-radius",
        "1",
        "--fence",
        "0,1,2,3",
        "--candidate",
        "4",
        "1",
        "all",
        "--max-activations",
        "1",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_coverage(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(
        checked.source,
        holos_tda_check::VerifiedCoverageSource::Affine
    );
    assert_eq!(checked.states, 3);
    assert_eq!(checked.total_cost, Some(1));
}
