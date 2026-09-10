use super::*;

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
