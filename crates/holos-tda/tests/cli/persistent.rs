use std::fmt::Write as _;

use super::*;

const ESSENTIAL_GRAPH: &str = "0 1 1\n0 3 1\n1 2 1\n2 3 1\n";
const FINITE_GRAPH: &str = "0 1 1\n0 2 2\n0 3 1\n1 2 1\n1 3 2\n2 3 1\n";
const LOWER_DISTANCE_BASE: &str = "1 2 1 1 2 1\n";
const LOWER_DISTANCE_CHANGED: &str = "1 2 1 1 2.5 1\n";

fn essential_graph(name: &str) -> TempFile {
    TempFile::new(name, ESSENTIAL_GRAPH)
}

fn finite_graph(name: &str) -> TempFile {
    TempFile::new(name, FINITE_GRAPH)
}

#[test]
fn persistent_class_cli_hands_a_finite_witness_to_the_checker() {
    let graph = finite_graph("persistent_class_finite.spr");
    let artifact = TempFile::new("persistent_class_finite.hspc", "");
    let output = run(&[
        "persistent-class",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "2",
        "--modulus",
        "5",
        "--space",
        "0",
        "--basis",
        "0",
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_persistent_class(&bytes));
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.interval().birth, 1.0);
    assert_eq!(checked.interval().death, 2.0);
    assert!(!checked.cycle().is_empty());
    assert!(!checked.bounding_chain().is_empty());
}

#[test]
fn persistent_class_cli_hands_an_essential_witness_to_the_checker() {
    let graph = essential_graph("persistent_class_essential.spr");
    let artifact = TempFile::new("persistent_class_essential.hspc", "");
    let output = run(&[
        "persistent-class",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "5",
        "--space",
        "0",
        "--basis",
        "0",
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_persistent_class(&bytes));
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert!(checked.interval().death.is_infinite());
    assert!(checked.bounding_chain().is_empty());
}

#[test]
fn persistent_class_cli_keeps_all_lower_distance_edges_in_the_source_binding() {
    let base = TempFile::new("persistent_lower_base.lower", LOWER_DISTANCE_BASE);
    let changed = TempFile::new("persistent_lower_changed.lower", LOWER_DISTANCE_CHANGED);
    let base_artifact = TempFile::new("persistent_lower_base.hspc", "");
    let changed_artifact = TempFile::new("persistent_lower_changed.hspc", "");
    for (input, output) in [
        (base.path(), base_artifact.path()),
        (changed.path(), changed_artifact.path()),
    ] {
        let result = run(&[
            "persistent-class",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--format",
            "lower-distance",
            "--threshold",
            "1",
            "--modulus",
            "5",
            "--space",
            "0",
            "--basis",
            "0",
        ]);
        assert!(result.status.success(), "stderr: {}", stderr(&result));
    }
    let base_bytes = std::fs::read(base_artifact.path()).unwrap();
    let changed_bytes = std::fs::read(changed_artifact.path()).unwrap();
    assert_ne!(base_bytes, changed_bytes);
    let base_checked = verify_persistent_class(&base_bytes, ProofLimits::default()).unwrap();
    let changed_checked = verify_persistent_class(&changed_bytes, ProofLimits::default()).unwrap();
    assert_eq!(base_checked.interval(), changed_checked.interval());
    assert_eq!(base_checked.source().len(), 6);
    assert_eq!(changed_checked.source().len(), 6);
    assert_eq!(base_checked.source()[4].value(), 2.0);
    assert_eq!(changed_checked.source()[4].value(), 2.5);
}

#[test]
fn persistent_circular_cli_accepts_a_supplied_mod_two_lift() {
    let graph = essential_graph("persistent_circular_mod_two.spr");
    let class_artifact = TempFile::new("persistent_circular_mod_two.hspc", "");
    let class_output = run(&[
        "persistent-class",
        graph.path().to_str().unwrap(),
        class_artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "2",
        "--space",
        "0",
        "--basis",
        "0",
    ]);
    assert!(
        class_output.status.success(),
        "stderr: {}",
        stderr(&class_output)
    );
    let checked_class = verify_persistent_class(
        &std::fs::read(class_artifact.path()).unwrap(),
        ProofLimits::default(),
    )
    .unwrap();
    let mut lift_text = String::new();
    for term in checked_class.cocycle() {
        writeln!(
            lift_text,
            "{} {} {}",
            term.u(),
            term.v(),
            term.coefficient()
        )
        .unwrap();
    }
    let lift = TempFile::new("persistent_circular_mod_two.integral", &lift_text);
    let phases = TempFile::new("persistent_circular_mod_two.phase", "");
    let artifact = TempFile::new("persistent_circular_mod_two.hsph", "");
    let output = run(&[
        "persistent-circular",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "2",
        "--space",
        "0",
        "--basis",
        "0",
        "--integral-lift",
        lift.path().to_str().unwrap(),
        "--phases",
        phases.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_persistent_coordinate(&bytes));
    let checked = verify_persistent_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.modulus(), 2);
    assert_eq!(checked.class().interval().death, f64::INFINITY);
    assert!(checked.class().bounding_chain().is_empty());
    assert_eq!(
        std::fs::read_to_string(phases.path())
            .unwrap()
            .lines()
            .count(),
        4
    );
}

#[test]
fn persistent_circular_cli_keeps_old_output_when_lift_parsing_fails() {
    let graph = essential_graph("persistent_circular_bad_lift.spr");
    let lift = TempFile::new("persistent_circular_bad_lift.integral", "1 0 1\n");
    let artifact = TempFile::new("persistent_circular_bad_lift.hsph", "previous output");
    let output = run(&[
        "persistent-circular",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "2",
        "--space",
        "0",
        "--basis",
        "0",
        "--integral-lift",
        lift.path().to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn persistent_class_cli_rejects_lexical_output_aliases_before_writing() {
    let graph = finite_graph("persistent_class_output_alias.spr");
    let artifact = TempFile::new("persistent_class_output_alias.hspc", "previous output");
    let record = artifact
        .path()
        .parent()
        .unwrap()
        .join(".")
        .join(artifact.path().file_name().unwrap());
    let output = run(&[
        "persistent-class",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--space",
        "0",
        "--basis",
        "0",
        "--record",
        record.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("refer to the same file"));
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn persistent_circular_cli_rejects_aliasing_artifact_and_phase_paths() {
    let graph = essential_graph("persistent_circular_output_alias.spr");
    let artifact = TempFile::new("persistent_circular_output_alias.hsph", "previous output");
    let phases = artifact
        .path()
        .parent()
        .unwrap()
        .join(".")
        .join(artifact.path().file_name().unwrap());
    let output = run(&[
        "persistent-circular",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "2",
        "--space",
        "0",
        "--basis",
        "0",
        "--phases",
        phases.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("refer to the same file"));
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn persistent_circular_cli_rejects_aliasing_record_and_phase_paths() {
    let graph = essential_graph("persistent_circular_record_phase_alias.spr");
    let artifact = TempFile::new("persistent_circular_record_phase.hsph", "");
    let record = TempFile::new("persistent_circular_record_phase.record", "previous record");
    let output = run(&[
        "persistent-circular",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--threshold",
        "1",
        "--modulus",
        "2",
        "--space",
        "0",
        "--basis",
        "0",
        "--record",
        record.path().to_str().unwrap(),
        "--phases",
        record.path().to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("refer to the same file"));
    assert_eq!(
        std::fs::read_to_string(record.path()).unwrap(),
        "previous record"
    );
    assert_eq!(std::fs::read_to_string(artifact.path()).unwrap(), "");
}

#[cfg(unix)]
#[test]
fn persistent_class_cli_rejects_record_symlink_to_artifact() {
    use std::os::unix::fs::symlink;

    let graph = finite_graph("persistent_class_output_symlink.spr");
    let artifact = TempFile::new("persistent_class_output_symlink.hspc", "previous output");
    let record = TempFile::new("persistent_class_output_symlink.record", "");
    std::fs::remove_file(record.path()).unwrap();
    symlink(artifact.path(), record.path()).unwrap();
    let output = run(&[
        "persistent-class",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--space",
        "0",
        "--basis",
        "0",
        "--record",
        record.path().to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("refer to the same file"));
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}
