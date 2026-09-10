use super::*;

#[test]
fn representatives_and_atlas_verify_end_to_end() {
    let input = TempFile::new("proof_square.csv", "0 0\n1 0\n1 1\n0 1\n");
    let representatives = TempFile::new("classes.json", "");
    let proof = TempFile::new("run.hatlas", "");
    let input_path = input.path().to_str().unwrap();
    let proof_path = proof.path().to_str().unwrap();
    let out = run(&[
        input_path,
        "--dim",
        "1",
        "--threshold",
        "2",
        "--threads",
        "3",
        "--collapse-edges",
        "--collapse-schedule",
        "rounds",
        "--representatives",
        representatives.path().to_str().unwrap(),
        "--atlas",
        proof_path,
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stderr(&out).contains("H1 representatives: wrote 1 classes"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("persistence atlas: wrote"),
        "{}",
        stderr(&out)
    );
    let json = std::fs::read_to_string(representatives.path()).unwrap();
    assert!(json.contains("\"format\": \"holos-h1-class-spaces-v2\""));
    assert!(json.contains("\"multiplicity\": 1"));
    assert!(json.contains("\"terms\": ["));
    assert!(json.contains("\"schema\": \"holos-persistent-class-source-v1\""));

    let verified = run(&["verify-atlas", input_path, proof_path]);
    assert!(verified.status.success(), "stderr: {}", stderr(&verified));
    assert!(
        stdout(&verified).contains("1 H1 class spaces, 1 basis classes"),
        "{}",
        stdout(&verified)
    );

    let other = TempFile::new("proof_other.csv", "0 0\n2 0\n2 2\n0 2\n");
    let rejected = run(&["verify-atlas", other.path().to_str().unwrap(), proof_path]);
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("binding"),
        "{}",
        stderr(&rejected)
    );
}

#[test]
fn program_and_intervention_verify_end_to_end() {
    let input = TempFile::new("program_square.csv", "0 0\n1 0\n1 1\n0 1\n");
    let program = TempFile::new("run.hprogram", "");
    let intervention = TempFile::new("run.hintervention", "");
    let input_path = input.path().to_str().unwrap();
    let program_path = program.path().to_str().unwrap();
    let intervention_path = intervention.path().to_str().unwrap();

    let built = run(&[
        input_path,
        "--dim",
        "1",
        "--threshold",
        "2",
        "--program",
        program_path,
    ]);
    assert!(built.status.success(), "stderr: {}", stderr(&built));
    assert!(
        stderr(&built).contains("persistence program: wrote"),
        "{}",
        stderr(&built)
    );

    let checked = run(&["verify-program", input_path, program_path]);
    assert!(checked.status.success(), "stderr: {}", stderr(&checked));
    assert!(
        stdout(&checked).contains("1 cyclic atoms"),
        "{}",
        stdout(&checked)
    );

    let produced = run(&[
        "intervene",
        input_path,
        program_path,
        intervention_path,
        "--space",
        "0",
        "--before",
        "1.2",
    ]);
    assert!(produced.status.success(), "stderr: {}", stderr(&produced));
    assert!(
        stdout(&produced).contains("certified H1 intervention"),
        "{}",
        stdout(&produced)
    );

    let checked = run(&["verify-intervention", intervention_path]);
    assert!(checked.status.success(), "stderr: {}", stderr(&checked));
    assert!(
        stdout(&checked).contains("verified H1 intervention"),
        "{}",
        stdout(&checked)
    );
}

#[test]
fn program_trace_verifier_replays_local_updates() {
    let path = TempFile::new("program_trace.hdelta", "");
    let graph = |tail: f64| {
        holos_tda::SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 1.1),
                (2, 3, 1.2),
                (0, 3, 1.3),
                (0, 2, 2.0),
                (1, 3, 2.1),
                (3, 4, 1.0),
                (4, 5, 1.1),
                (5, 6, 1.2),
                (3, 6, tail),
                (3, 5, 2.0),
                (4, 6, 2.1),
            ],
        )
        .unwrap()
    };
    let initial = graph(1.3);
    let update = graph(1.31);
    let artifact = holos_tda::ProgramTraceArtifact::build(
        &initial,
        &[update],
        &holos_tda::RipsParams::new(1),
        holos_tda::CertificateLimits::default(),
    )
    .unwrap();
    std::fs::write(path.path(), artifact.encode().unwrap()).unwrap();

    let checked = run(&["verify-program-trace", path.path().to_str().unwrap()]);
    assert!(checked.status.success(), "stderr: {}", stderr(&checked));
    assert!(stdout(&checked).contains("1 steps"), "{}", stdout(&checked));
}

#[test]
fn trajectory_verifier_checks_reuse_and_region_boundaries() {
    let path = TempFile::new("trajectory.htrace", "");
    let graph = |weights: [f64; 6]| {
        holos_tda::SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, weights[0]),
                (0, 2, weights[1]),
                (0, 3, weights[2]),
                (1, 2, weights[3]),
                (1, 3, weights[4]),
                (2, 3, weights[5]),
            ],
        )
        .unwrap()
    };
    let initial = graph([1.0, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let reuse = graph([1.01, 2.01, 1.11, 1.21, 2.11, 1.31]);
    let boundary = graph([2.2, 2.0, 1.1, 1.2, 2.1, 1.3]);
    let artifact = holos_tda::TrajectoryArtifact::build(
        &initial,
        &[reuse, boundary],
        &holos_tda::RipsParams::new(1),
        holos_tda::CertificateLimits::default(),
    )
    .unwrap();
    std::fs::write(path.path(), artifact.encode().unwrap()).unwrap();

    let verified = run(&["verify-trajectory", path.path().to_str().unwrap()]);
    assert!(verified.status.success(), "stderr: {}", stderr(&verified));
    assert!(
        stdout(&verified).contains("2 steps, 1 reused"),
        "{}",
        stdout(&verified)
    );
}
