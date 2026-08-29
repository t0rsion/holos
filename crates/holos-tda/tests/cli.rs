//! End-to-end CLI tests against the built binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use holos_tda::collapse::{
    CollapsePortfolioArtifact, CollapsePortfolioDecodeLimits, CollapsePortfolioLimits,
};
use holos_tda::{
    CertificateLimits, DistributedInterfaceManifest, DurableInterfaceStore,
    RelativeInterfaceCertificate, RipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{
    IndexProofState, ProofLimits, is_cohomology_intervention, is_coverage,
    is_geometry_bound_coverage, is_kinetic_zigzag, is_relative_interface, is_synthesis,
    verify_cohomology_intervention, verify_coverage, verify_distributed_interface,
    verify_geometry_bound_coverage, verify_kinetic_zigzag, verify_relative_interface,
    verify_synthesis,
};

const BIN: &str = env!("CARGO_BIN_EXE_holos");

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("holos_cli_test_{}_{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        TempFile(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn new_bytes(name: &str, contents: &[u8]) -> Self {
        let path =
            std::env::temp_dir().join(format!("holos_cli_test_{}_{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        TempFile(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to launch holos")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
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

// The section printed for one dimension: everything between its header and
// the next header, or the end of output.
fn dim_section(text: &str, dim: usize) -> String {
    let header = format!("persistence intervals in dim {dim}:\n");
    let rest = text
        .split_once(&header)
        .unwrap_or_else(|| panic!("missing dim {dim} header: {text}"))
        .1;
    rest.split("persistence intervals")
        .next()
        .unwrap()
        .to_string()
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

// The unsigned integers of a stderr line, in order.
fn numbers(line: &str) -> Vec<usize> {
    line.split_whitespace()
        .filter_map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<usize>()
                .ok()
        })
        .collect()
}

fn line_starting(text: &str, prefix: &str) -> String {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?}: {text}"))
        .to_string()
}

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
    assert!(json.contains("\"format\": \"holos-h1-class-spaces-v1\""));
    assert!(json.contains("\"multiplicity\":1"));
    assert!(json.contains("\"terms\":["));

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

#[test]
fn engine_setting_keeps_the_output() {
    // A square: sides 1, diagonals sqrt(2). Every engine must print the
    // same diagram, and an unknown name must be refused.
    let f = TempFile::new("engine.csv", "0 0\n1 0\n1 1\n0 1\n");
    let path = f.path().to_str().unwrap();
    let expected = stdout(&run(&[path, "--dim", "1"]));
    for engine in ["auto", "dense", "sparse"] {
        let out = run(&[path, "--dim", "1", "--engine", engine]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert_eq!(stdout(&out), expected, "engine {engine}");
    }
    let out = run(&[path, "--engine", "quantum"]);
    assert!(!out.status.success());
}

#[test]
fn dense_storage_setting_keeps_the_output() {
    // Every storage form must print the same diagram on every engine, and
    // an unknown name must be refused.
    let f = TempFile::new("storage.csv", "0 0\n1 0\n1 1\n0 1\n");
    let path = f.path().to_str().unwrap();
    let expected = stdout(&run(&[path, "--dim", "1"]));
    for engine in ["auto", "dense", "sparse"] {
        for storage in ["auto", "compact", "square"] {
            let out = run(&[
                path,
                "--dim",
                "1",
                "--engine",
                engine,
                "--dense-storage",
                storage,
            ]);
            assert!(out.status.success(), "stderr: {}", stderr(&out));
            assert_eq!(stdout(&out), expected, "engine {engine}, storage {storage}");
        }
    }
    let out = run(&[path, "--dense-storage", "triangular"]);
    assert!(!out.status.success());
}

#[test]
fn composite_modulus_is_rejected() {
    let f = TempFile::new("mod4.lower", "1\n1 1\n");
    let out = run(&[f.path().to_str().unwrap(), "--modulus", "4"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("prime"), "{}", stderr(&out));
}

#[test]
fn sparse_format_end_to_end() {
    // A 4-cycle with unit edges and no diagonals: three merges at 1, one
    // essential component, one essential H1 class. Nothing ever fills the
    // loop.
    let f = TempFile::new("cycle.sparse", "0 1 1.0\n1 2 1.0\n2 3 1.0\n0 3 1.0\n");
    let out = run(&[
        f.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "1",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "persistence intervals in dim 0:\n [0,1)\n [0,1)\n [0,1)\n [0, )\n\
         persistence intervals in dim 1:\n [1, )\n"
    );
}

#[test]
fn prove_writes_one_dag_for_the_complete_trajectory() {
    let initial = TempFile::new(
        "proof_initial.sparse",
        "0 1 1.0\n1 2 1.0\n2 3 1.0\n0 3 1.0\n3 4 1.0\n4 5 1.0\n5 6 1.0\n3 6 1.0\n",
    );
    let update = TempFile::new(
        "proof_update.sparse",
        "0 1 1.1\n1 2 1.0\n2 3 1.0\n0 3 1.0\n3 4 1.0\n4 5 1.0\n5 6 1.0\n3 6 1.0\n",
    );
    let output = TempFile::new("trajectory.hpf", "");
    let out = run(&[
        "prove",
        initial.path().to_str().unwrap(),
        output.path().to_str().unwrap(),
        update.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("wrote 2 snapshots"),
        "{}",
        stdout(&out)
    );
    let bytes = std::fs::read(output.path()).unwrap();
    assert!(bytes.starts_with(b"HOLOSPF\0"));
}

#[test]
fn index_writes_an_initial_checkpoint_and_warm_record() {
    let initial = TempFile::new(
        "index_initial.sparse",
        "0 1 0.25\n0 2 1.0\n1 2 1.5\n0 3 1.2\n1 3 1.7\n0 4 1.1\n1 4 1.6\n0 5 1.3\n1 5 1.8\n",
    );
    let update = TempFile::new(
        "index_update.sparse",
        "0 1 0.25\n0 2 1.01\n1 2 1.5\n0 3 1.2\n1 3 1.7\n0 4 1.1\n1 4 1.6\n0 5 1.3\n1 5 1.8\n",
    );
    let snapshot = TempFile::new("index.hip", "");
    let delta = TempFile::new("index.hdp", "");
    let out = run(&[
        "index",
        initial.path().to_str().unwrap(),
        snapshot.path().to_str().unwrap(),
        "--update",
        update.path().to_str().unwrap(),
        "--record",
        delta.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("one initial checkpoint"),
        "{}",
        stdout(&out)
    );
    assert!(
        std::fs::read(snapshot.path())
            .unwrap()
            .starts_with(b"HOLOSIP\0")
    );
    assert!(
        std::fs::read(delta.path())
            .unwrap()
            .starts_with(b"HOLOSDP\0")
    );
}

#[test]
fn index_cli_writes_an_independently_checked_h2_checkpoint() {
    let graph = TempFile::new(
        "index_h2.sparse",
        "0 2 1.02\n0 3 1.03\n0 4 1.04\n0 5 1.05\n1 2 1.03\n1 3 1.04\n1 4 1.05\n1 5 1.06\n2 4 1.06\n2 5 1.07\n3 4 1.07\n3 5 1.08\n",
    );
    let snapshot = TempFile::new("index_h2.hip", "");
    let out = run(&[
        "index",
        graph.path().to_str().unwrap(),
        snapshot.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "2",
        "--modulus",
        "5",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("through dimension 2"));
    let bytes = std::fs::read(snapshot.path()).unwrap();
    let (_, checked) = IndexProofState::verify_snapshot(&bytes, ProofLimits::default()).unwrap();
    assert!(checked.triangle_columns_checked > 0);
}

#[test]
fn interface_cli_writes_an_independently_checked_relative_core() {
    let graph = TempFile::new(
        "relative.sparse",
        "0 1 1.0\n1 2 1.0\n2 3 1.0\n0 3 1.0\n0 4 2.0\n1 4 2.0\n2 5 2.5\n3 5 2.5\n",
    );
    let artifact = TempFile::new("relative.hri", "");
    let out = run(&[
        "interface",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "2",
        "--modulus",
        "5",
        "--protect",
        "0",
        "--protect",
        "1",
        "--protect",
        "2",
        "--protect",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_relative_interface(&bytes));
    let checked = verify_relative_interface(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.max_dim, 2);
    assert!(checked.input_cells >= checked.core_cells);
}

#[test]
fn merge_interfaces_cli_commits_an_independently_checked_manifest() {
    let child = |labels: &[usize], side: usize| {
        let graph = SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (side, 4, 2.0),
                ((side + 1) % 4, 4, 2.0),
            ],
        )
        .unwrap();
        RelativeInterfaceCertificate::build_labeled(
            &graph,
            labels,
            &RipsParams::new(2).with_modulus(3),
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap()
        .encode(CertificateLimits::default())
        .unwrap()
    };
    let first = TempFile::new_bytes("first.hri", &child(&[0, 1, 2, 3, 4], 0));
    let second = TempFile::new_bytes("second.hri", &child(&[0, 1, 2, 3, 5], 1));
    let manifest_file = TempFile::new("distributed.hdm", "");
    let result_file = TempFile::new("distributed.hri", "");
    let store_path = std::env::temp_dir().join(format!(
        "holos_cli_test_{}_distributed_store",
        std::process::id()
    ));
    if store_path.exists() {
        std::fs::remove_dir_all(&store_path).unwrap();
    }
    let out = run(&[
        "merge-interfaces",
        store_path.to_str().unwrap(),
        manifest_file.path().to_str().unwrap(),
        result_file.path().to_str().unwrap(),
        first.path().to_str().unwrap(),
        second.path().to_str().unwrap(),
        "--separator",
        "0",
        "--separator",
        "1",
        "--separator",
        "2",
        "--separator",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let manifest_bytes = std::fs::read(manifest_file.path()).unwrap();
    let manifest = DistributedInterfaceManifest::decode(&manifest_bytes, 1 << 30).unwrap();
    let store = DurableInterfaceStore::open(&store_path).unwrap();
    let mut ids: std::collections::BTreeSet<_> = manifest.shards().iter().copied().collect();
    ids.extend(manifest.folds().iter().copied());
    ids.insert(manifest.result());
    let objects: Vec<_> = ids
        .into_iter()
        .map(|id| store.get(id, 1 << 30).unwrap())
        .collect();
    let checked =
        verify_distributed_interface(&manifest_bytes, &objects, ProofLimits::default()).unwrap();
    assert_eq!(checked.shards, 2);
    assert_eq!(
        std::fs::read(result_file.path()).unwrap(),
        store.get(manifest.result(), 1 << 30).unwrap()
    );
    std::fs::remove_dir_all(&store_path).unwrap();
}

fn octahedral_sphere_file(name: &str) -> TempFile {
    let mut text = String::new();
    for u in 0..6 {
        for v in u + 1..6 {
            if u / 2 != v / 2 {
                text.push_str(&format!("{u} {v} 1\n"));
            }
        }
    }
    TempFile::new(name, &text)
}

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

#[test]
fn version_reports_build_identity() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "missing crate version: {text}"
    );
    assert!(
        text.contains("release") || text.contains("debug"),
        "missing build profile: {text}"
    );
    let hash = text
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(','))
        .map(|(h, _)| h)
        .unwrap_or_else(|| panic!("no '(hash, profile)' in: {text}"));
    // Source archives without git metadata report "unknown".
    assert!(
        hash == "unknown" || (hash.len() == 12 && hash.chars().all(|c| c.is_ascii_hexdigit())),
        "git hash neither 12-hex nor unknown: {text}"
    );
}

#[test]
fn malformed_input_fails_with_useful_message() {
    let f = TempFile::new("bad.csv", "1.0 2.0\n1.0 oops\n");
    let out = run(&[f.path().to_str().unwrap()]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("not a number"), "{err}");
    assert!(err.contains(":2:"), "missing line number: {err}");
}

#[test]
fn negative_threshold_is_rejected() {
    let f = TempFile::new("neg_thresh.lower", "1\n1 1\n");
    let out = run(&[f.path().to_str().unwrap(), "--threshold=-1"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("threshold"), "{}", stderr(&out));
}

#[test]
fn nan_threshold_is_rejected() {
    let f = TempFile::new("nan_thresh.lower", "1\n1 1\n");
    let out = run(&[f.path().to_str().unwrap(), "--threshold=NaN"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("threshold"), "{}", stderr(&out));
}

#[test]
fn empty_input_still_validates_threshold() {
    let file = TempFile::new("empty_cloud.csv", "");
    let out = run(&[file.path().to_str().unwrap(), "--threshold=-1"]);
    assert!(!out.status.success());
    let out = run(&[file.path().to_str().unwrap(), "--threshold", "NaN"]);
    assert!(!out.status.success());
}

#[test]
fn huge_dim_is_bounded_by_the_point_count() {
    let file = TempFile::new("four_points.csv", "0,0\n1,0\n0,1\n1,1\n");
    let out = run(&[
        file.path().to_str().unwrap(),
        "--dim",
        "18446744073709551615",
    ]);
    assert!(out.status.success());
    // Four points support dimensions 0..=3 only.
    assert_eq!(
        stdout(&out)
            .lines()
            .filter(|l| l.starts_with("persistence intervals in dim"))
            .count(),
        4
    );
}
