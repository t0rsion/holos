//! End-to-end CLI tests against the built binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    // edges, small enough to stay a quick run.
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
fn engine_setting_keeps_the_output() {
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
    // Provenance-free source archives legitimately report "unknown".
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
