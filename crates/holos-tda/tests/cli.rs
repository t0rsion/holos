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
