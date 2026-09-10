use super::*;

#[test]
fn collapse_edges_keeps_the_diagram_and_reports_stats() {
    // Nine points on a 3x3 grid: dense enough that the collapse removes
    // edges, small enough to stay a quick run. The diagram may not move.
    let f = TempFile::new("grid.csv", "0 0\n1 0\n2 0\n0 1\n1 1\n2 1\n0 2\n1 2\n2 2\n");
    let path = f.path().to_str().unwrap();
    let plain = run(&[path, "--dim", "2"]);
    let collapsed = run(&[path, "--dim", "2", "--collapse-edges"]);
    assert!(plain.status.success(), "stderr: {}", stderr(&plain));
    assert!(collapsed.status.success(), "stderr: {}", stderr(&collapsed));
    assert_eq!(
        stdout(&plain),
        stdout(&collapsed),
        "--collapse-edges changed the diagram"
    );
    assert!(
        !stderr(&plain).contains("collapse:"),
        "collapse statistics without the flag: {}",
        stderr(&plain)
    );

    let err = stderr(&collapsed);
    let kept = line_starting(&err, "collapse: kept");
    let n = numbers(&kept);
    assert_eq!(n.len(), 4, "unexpected statistics line: {kept}");
    assert_eq!(
        n[0] + n[2],
        n[1],
        "kept plus removed must be the input: {kept}"
    );
    assert!(n[2] > 0, "the grid must yield removals: {kept}");
    assert!(n[3] >= 2, "a removal needs a following empty pass: {kept}");
    // The default schedule is the serial version 1 schedule.
    assert!(kept.ends_with("passes"), "expected pass wording: {kept}");

    let detail = line_starting(&err, "collapse detail:");
    assert!(detail.contains("edge tests"), "{detail}");
    assert!(detail.contains("witness segments"), "{detail}");
    assert_eq!(
        numbers(&detail).len(),
        3,
        "unexpected detail line: {detail}"
    );

    let bare = run(&[path, "--dim", "2", "--collapse-schedule", "rounds"]);
    assert!(
        !bare.status.success(),
        "--collapse-schedule without --collapse-edges must be rejected"
    );
    assert!(
        stderr(&bare).contains("requires --collapse-edges"),
        "{}",
        stderr(&bare)
    );

    // Every schedule keeps the diagram at every thread count. The ordered
    // schedule reports passes; the rounds schedule reports rounds. At four
    // threads the parallel schedules test a different number of edges
    // than the serial one on this grid, which shows the flag reached the
    // pipeline.
    let serial_detail = detail.clone();
    for schedule in ["serial", "ordered", "rounds"] {
        for threads in ["1", "4"] {
            let out = run(&[
                path,
                "--dim",
                "2",
                "--collapse-edges",
                "--collapse-schedule",
                schedule,
                "--threads",
                threads,
            ]);
            assert!(out.status.success(), "stderr: {}", stderr(&out));
            assert_eq!(
                stdout(&plain),
                stdout(&out),
                "schedule {schedule} at {threads} threads changed the diagram"
            );
            let kept = line_starting(&stderr(&out), "collapse: kept");
            let unit = if schedule == "rounds" {
                "rounds"
            } else {
                "passes"
            };
            assert!(
                kept.ends_with(unit),
                "schedule {schedule}: expected {unit} wording: {kept}"
            );
            let this_detail = line_starting(&stderr(&out), "collapse detail:");
            if schedule == "serial" {
                assert_eq!(this_detail, serial_detail, "serial detail must not move");
            } else if threads == "4" {
                assert_ne!(
                    this_detail, serial_detail,
                    "schedule {schedule} at 4 threads ran the serial collapse"
                );
            }
        }
    }
}

#[test]
fn adaptive_collapse_reports_complete_and_budget_limited_runs() {
    let f = TempFile::new(
        "adaptive_grid.csv",
        "0 0\n1 0\n2 0\n0 1\n1 1\n2 1\n0 2\n1 2\n2 2\n",
    );
    let path = f.path().to_str().unwrap();
    let plain = run(&[path, "--dim", "2"]);

    let complete = run(&[
        path,
        "--dim",
        "2",
        "--collapse-edges",
        "--collapse-schedule",
        "adaptive",
        "--collapse-objective",
        "h2",
    ]);
    assert!(complete.status.success(), "stderr: {}", stderr(&complete));
    assert_eq!(stdout(&complete), stdout(&plain));
    let complete_err = stderr(&complete);
    assert!(
        line_starting(&complete_err, "collapse: kept").ends_with("passes"),
        "{complete_err}"
    );
    assert!(
        complete_err.contains("complete fixed point"),
        "{complete_err}"
    );

    let partial = run(&[
        path,
        "--dim",
        "2",
        "--collapse-edges",
        "--collapse-schedule",
        "adaptive",
        "--collapse-work-limit",
        "0",
    ]);
    assert!(partial.status.success(), "stderr: {}", stderr(&partial));
    assert_eq!(stdout(&partial), stdout(&plain));
    assert!(
        stderr(&partial).contains("budget-limited partial collapse, 0 work units"),
        "{}",
        stderr(&partial)
    );

    for args in [
        vec![path, "--collapse-objective", "h1"],
        vec![path, "--collapse-work-limit", "1"],
    ] {
        let out = run(&args);
        assert!(!out.status.success());
        assert!(
            stderr(&out).contains("require --collapse-schedule adaptive"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn portable_collapse_artifact_verifies_in_a_separate_command() {
    let input = TempFile::new("artifact_k4.lower", "1\n1 1\n1 1 1\n");
    let artifact = TempFile::new("artifact.hcol", "");
    let input_path = input.path().to_str().unwrap();
    let artifact_path = artifact.path().to_str().unwrap();

    let produce = run(&[
        input_path,
        "--dim",
        "2",
        "--collapse-edges",
        "--collapse-schedule",
        "adaptive",
        "--collapse-certificate",
        artifact_path,
    ]);
    assert!(produce.status.success(), "stderr: {}", stderr(&produce));
    assert!(
        stderr(&produce).contains("collapse artifact: wrote"),
        "{}",
        stderr(&produce)
    );

    let verify = run(&["verify-collapse", input_path, artifact_path]);
    assert!(verify.status.success(), "stderr: {}", stderr(&verify));
    assert!(
        stdout(&verify).contains("verified collapse artifact: algorithm version 3"),
        "{}",
        stdout(&verify)
    );

    let other = TempFile::new("artifact_other.lower", "2\n2 2\n2 2 2\n");
    let wrong = run(&[
        "verify-collapse",
        other.path().to_str().unwrap(),
        artifact_path,
    ]);
    assert!(!wrong.status.success());
    assert!(
        stderr(&wrong).contains("artifact binding"),
        "{}",
        stderr(&wrong)
    );

    let bare = run(&[input_path, "--collapse-certificate", artifact_path]);
    assert!(!bare.status.success());
    assert!(
        stderr(&bare).contains("requires --collapse-edges"),
        "{}",
        stderr(&bare)
    );
}
