use super::*;

#[test]
fn bipersistence_cli_writes_a_checked_class_aware_module() {
    let input = TempFile::new(
        "bipersistence.sparse",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n0 2 2\n1 3 2\n",
    );
    let artifact = TempFile::new_bytes("bipersistence.hbp", b"");
    let report = TempFile::new_bytes("bipersistence.json", b"");
    let out = run(&[
        "bipersistence",
        input.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--rectangle",
        "1",
        "1",
        "2",
        "3",
        "--class",
        "1",
        "1",
        "0",
        "--circular",
        "--report",
        report.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_bipersistence(&bytes));
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.nodes, 12);
    assert_eq!(checked.rectangles, 1);
    assert_eq!(checked.class_atlases, 1);
    assert_eq!(checked.circular_families, 1);
    let report = std::fs::read_to_string(report.path()).unwrap();
    assert!(report.contains("\"format\": \"holos-degree-rips-report-v1\""));
    assert!(report.contains("\"rectangles\":"));
    assert!(report.contains("\"kind\":\"no_extension\""));
}

#[test]
fn bipersistence_cli_accepts_a_cocycle_class() {
    let input = TempFile::new(
        "bipersistence_cocycle.sparse",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n",
    );
    let cocycle = TempFile::new("bipersistence_cocycle.txt", "0 1 1\n");
    let artifact = TempFile::new_bytes("bipersistence_cocycle.hbp", b"");
    let out = run(&[
        "bipersistence",
        input.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--class-cocycle",
        "1",
        "1",
        cocycle.path().to_str().unwrap(),
        "--circular",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.class_atlases, 1);
    assert_eq!(checked.circular_families, 1);
}

#[test]
fn bipersistence_cli_checks_a_declared_finite_grid() {
    let input = TempFile::new(
        "bipersistence_grid.sparse",
        "0 1 1\n1 2 1\n2 3 1\n0 3 1\n0 2 2\n1 3 2\n",
    );
    let artifact = TempFile::new_bytes("bipersistence_grid.hbp", b"");
    let region = TempFile::new("bipersistence_region.txt", "0 1\n1 0\n1 1\n");
    let out = run(&[
        "bipersistence",
        input.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--scale",
        "1",
        "--region",
        region.path().to_str().unwrap(),
        "--scale",
        "2",
        "--minimum-degree",
        "2",
        "--minimum-degree",
        "0",
        "--rectangle",
        "0",
        "0",
        "1",
        "1",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.scales, 2);
    assert_eq!(checked.density_levels, 2);
    assert_eq!(checked.nodes, 4);
    assert_eq!(checked.rectangles, 1);
    assert_eq!(checked.regions, 1);
}

#[test]
fn point_cloud_end_to_end() {
    // Unit square: sides 1, diagonals sqrt(2). The default threshold is the
    // enclosing radius, so H1 is the finite bar (1, sqrt(2)).
    let f = TempFile::new("square.csv", "0 0\n1 0\n1 1\n0 1\n");
    let out = run(&[f.path().to_str().unwrap(), "--dim", "1"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("persistence intervals in dim 0:"), "{text}");
    assert!(text.contains("persistence intervals in dim 1:"), "{text}");
    assert_eq!(text.matches(" [0,1)").count(), 3, "{text}");
    assert_eq!(text.matches(" [0, )").count(), 1, "{text}");
    assert!(text.contains(" [1,1.4142135623730951)"), "{text}");
}

#[test]
fn finite_point_threshold_uses_native_graph_construction() {
    let f = TempFile::new("native_square.csv", "0 0\n1 0\n1 1\n0 1\n");
    let out = run(&[
        f.path().to_str().unwrap(),
        "--dim",
        "1",
        "--threshold",
        "1.1",
        "--threads",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("4 edges"), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("KdTree point construction"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).contains(" [1, )"), "{}", stdout(&out));
}

#[test]
fn lower_distance_end_to_end_with_empty_top_dimension() {
    // Unit triangle: H1 and H2 are both empty. Their headers must still
    // print, in ripser-compatible syntax.
    let f = TempFile::new("triangle.lower", "1\n1 1\n");
    let out = run(&[f.path().to_str().unwrap(), "--dim", "2"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "persistence intervals in dim 0:\n [0,1)\n [0,1)\n [0, )\n\
         persistence intervals in dim 1:\npersistence intervals in dim 2:\n"
    );
}

#[test]
fn modulus_decides_projective_plane_torsion() {
    // Ripser's 13-vertex RP^2 triangulation: H1 and H2 exist over Z/2 only.
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/projective_plane.lower_distance_matrix"
    );
    let out = run(&[fixture, "--dim", "2", "--modulus", "2"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(dim_section(&text, 1), " [1,2)\n", "{text}");
    assert_eq!(dim_section(&text, 2), " [1,2)\n", "{text}");

    let out = run(&[fixture, "--dim", "2", "--modulus", "3"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(dim_section(&text, 1), "", "{text}");
    assert_eq!(dim_section(&text, 2), "", "{text}");
}
