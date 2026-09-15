use std::ffi::OsString;

use super::{parse_max_tolerance, run_circular, run_persistent_coordinate};

const CIRCULAR_ARTIFACT: &[u8] = include_bytes!("fixtures/circular-tolerance-1e-6.hcc");
const PERSISTENT_COORDINATE_ARTIFACT: &[u8] =
    include_bytes!("fixtures/persistent-tolerance-1e-6.hsph");

fn max_tolerance(value: &str) -> Vec<OsString> {
    vec![OsString::from("--max-tolerance"), OsString::from(value)]
}

#[test]
fn circular_runner_requires_an_explicit_tolerance_override() {
    let default = run_circular("holos-check", CIRCULAR_ARTIFACT, &[]);
    assert!(default.is_err(), "the default checker cap must reject 1e-6");

    let arguments = max_tolerance("1e-6");
    let explicit = run_circular("holos-check", CIRCULAR_ARTIFACT, &arguments);
    assert!(
        explicit.is_ok(),
        "the explicit checker cap should accept 1e-6"
    );
}

#[test]
fn persistent_coordinate_runner_requires_an_explicit_tolerance_override() {
    let default = run_persistent_coordinate("holos-check", PERSISTENT_COORDINATE_ARTIFACT, &[]);
    assert!(default.is_err(), "the default checker cap must reject 1e-6");

    let arguments = max_tolerance("1e-6");
    let explicit =
        run_persistent_coordinate("holos-check", PERSISTENT_COORDINATE_ARTIFACT, &arguments);
    assert!(
        explicit.is_ok(),
        "the explicit checker cap should accept 1e-6"
    );
}

#[test]
fn max_tolerance_requires_a_finite_positive_value() {
    for value in ["NaN", "-1", "0", "inf", "-inf"] {
        let arguments = max_tolerance(value);
        let error =
            parse_max_tolerance("holos-check", "a circular coordinate", &arguments).unwrap_err();
        assert!(
            error.contains("--max-tolerance must be a finite number greater than zero"),
            "{value}: {error}"
        );
    }
}

#[test]
fn runners_reject_nonfinite_and_negative_overrides() {
    for value in ["NaN", "-1"] {
        let arguments = max_tolerance(value);
        let circular_error =
            run_circular("holos-check", CIRCULAR_ARTIFACT, &arguments).unwrap_err();
        assert!(
            circular_error.contains("--max-tolerance must be a finite number greater than zero"),
            "{value}: {circular_error}"
        );
        let persistent_error =
            run_persistent_coordinate("holos-check", PERSISTENT_COORDINATE_ARTIFACT, &arguments)
                .unwrap_err();
        assert!(
            persistent_error.contains("--max-tolerance must be a finite number greater than zero"),
            "{value}: {persistent_error}"
        );
    }
}

#[test]
fn max_tolerance_defaults_to_the_checker_limit() {
    assert_eq!(
        parse_max_tolerance("holos-check", "a circular coordinate", &[]).unwrap(),
        1e-8
    );
}
