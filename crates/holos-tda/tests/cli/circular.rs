use super::*;
use std::fmt::Write as _;

const CYCLE_GRAPH: &str = "0 1 1\n1 2 1\n2 3 1\n3 4 1\n4 5 1\n5 6 1\n6 7 1\n0 7 1\n";

fn cycle_graph(name: &str) -> TempFile {
    TempFile::new(name, CYCLE_GRAPH)
}

#[test]
fn circular_cli_accepts_a_supplied_mod_two_integral_lift() {
    let graph = cycle_graph("circular_lift_mod_two.spr");
    let cocycle = TempFile::new("circular_lift_mod_two.cocycle", "0 1 1\n");
    let lift = TempFile::new("circular_lift_mod_two.integral", "0 1 1\n");
    let artifact = TempFile::new("circular_lift_mod_two.hcc", "");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        cocycle.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--at",
        "1",
        "--modulus",
        "2",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.modulus, 2);
    assert_eq!(checked.coordinates, 1);
    assert_eq!(checked.divisibilities, vec![1]);
}

#[test]
fn circular_cli_accepts_a_class_record_with_a_supplied_lift() {
    let graph = cycle_graph("circular_class_lift.spr");
    let classes = TempFile::new("circular_class_lift.json", "");
    let computed = run(&[
        graph.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "1",
        "--modulus",
        "2",
        "--representatives",
        classes.path().to_str().unwrap(),
    ]);
    assert!(computed.status.success(), "stderr: {}", stderr(&computed));

    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(classes.path()).unwrap()).unwrap();
    let records = document["spaces"][0]["basis"][0]["terms"]
        .as_array()
        .unwrap();
    let mut lift_text = String::new();
    for record in records {
        let row = record.as_array().unwrap();
        writeln!(
            lift_text,
            "{} {} {}",
            row[0].as_u64().unwrap(),
            row[1].as_u64().unwrap(),
            row[2].as_u64().unwrap()
        )
        .unwrap();
    }
    let lift = TempFile::new("circular_class_lift.integral", &lift_text);
    let artifact = TempFile::new("circular_class_lift.hcc", "");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        classes.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--class",
        "0",
        "0",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    let checked = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.modulus, 2);
    assert_eq!(checked.coordinates, 1);
}

#[test]
fn circular_cli_rejects_noncanonical_supplied_lift_rows() {
    let graph = cycle_graph("circular_lift_order.spr");
    let cocycle = TempFile::new("circular_lift_order.cocycle", "0 1 1\n");
    let lift = TempFile::new("circular_lift_order.integral", "0 7 1\n0 1 1\n");
    let artifact = TempFile::new("circular_lift_order.hcc", "");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        cocycle.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--at",
        "1",
        "--modulus",
        "2",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("integral lift terms are not in strict endpoint order"),
        "{}",
        stderr(&out)
    );
    assert!(std::fs::read(artifact.path()).unwrap().is_empty());
}

#[test]
fn circular_cli_rejects_mod_two_supplied_lift_continuation_before_writing() {
    let graph = cycle_graph("circular_lift_mod_two_continuation.spr");
    let cocycle = TempFile::new("circular_lift_mod_two_continuation.cocycle", "0 1 1\n");
    let lift = TempFile::new("circular_lift_mod_two_continuation.integral", "0 1 1\n");
    let artifact = TempFile::new("circular_lift_mod_two_continuation.hcc", "previous output");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        cocycle.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--at",
        "1",
        "--modulus",
        "2",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
        "--continue-to",
        graph.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains(
            "modulus 2 supplied lifts cannot be used with --continue-to; continuation computes the new coordinate automatically"
        ),
        "{message}"
    );
    assert!(
        !message.contains("supply a checked integral lift"),
        "{message}"
    );
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn circular_cli_rejects_mod_two_class_lift_continuation_before_writing() {
    let graph = cycle_graph("circular_class_lift_continuation.spr");
    let classes = TempFile::new("circular_class_lift_continuation.json", "");
    let computed = run(&[
        graph.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "1",
        "--modulus",
        "2",
        "--representatives",
        classes.path().to_str().unwrap(),
    ]);
    assert!(computed.status.success(), "stderr: {}", stderr(&computed));

    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(classes.path()).unwrap()).unwrap();
    let records = document["spaces"][0]["basis"][0]["terms"]
        .as_array()
        .unwrap();
    let mut lift_text = String::new();
    for record in records {
        let row = record.as_array().unwrap();
        writeln!(
            lift_text,
            "{} {} {}",
            row[0].as_u64().unwrap(),
            row[1].as_u64().unwrap(),
            row[2].as_u64().unwrap()
        )
        .unwrap();
    }
    let lift = TempFile::new("circular_class_lift_continuation.integral", &lift_text);
    let artifact = TempFile::new("circular_class_lift_continuation.hcc", "previous output");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        classes.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--class",
        "0",
        "0",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
        "--continue-to",
        graph.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains(
            "modulus 2 supplied lifts cannot be used with --continue-to; continuation computes the new coordinate automatically"
        ),
        "{message}"
    );
    assert!(
        !message.contains("supply a checked integral lift"),
        "{message}"
    );
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn circular_cli_rejects_a_nonclosed_supplied_lift_before_writing() {
    let graph = TempFile::new(
        "circular_lift_nonclosed_triangle.spr",
        "0 1 1\n1 2 1\n0 2 1\n0 3 1\n3 4 1\n4 5 1\n0 5 1\n",
    );
    let cocycle = TempFile::new("circular_lift_nonclosed_triangle.cocycle", "0 3 1\n");
    let lift = TempFile::new(
        "circular_lift_nonclosed_triangle.integral",
        "0 1 2\n0 3 1\n",
    );
    let artifact = TempFile::new("circular_lift_nonclosed_triangle.hcc", "previous output");
    let out = run(&[
        "circular",
        graph.path().to_str().unwrap(),
        cocycle.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--at",
        "1",
        "--modulus",
        "2",
        "--format",
        "sparse",
        "--integral-lift",
        lift.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("integral cocycle is not closed on triangle (0, 1, 2)"),
        "{message}"
    );
    assert_eq!(
        std::fs::read_to_string(artifact.path()).unwrap(),
        "previous output"
    );
}

#[test]
fn circular_help_describes_the_mod_two_continuation_limit() {
    let out = run(&["circular", "--help"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let help = stdout(&out);
    assert!(help.contains("modulus 2"), "{help}");
    assert!(help.contains("without --continue-to"), "{help}");
}
