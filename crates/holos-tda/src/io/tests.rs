use super::matrices::{CondensedSink, TripletSink};
use super::points::PointSink;
use super::scanner::{MIN_PARALLEL_BYTES, is_skipped, line_chunks, parse_workers, tokens};
use super::window::{Windows, count_newlines};
use super::*;
use crate::{DistanceMatrix, Error, Result};
use std::path::{Path, PathBuf};

mod output;
mod window;

/// The text parsers with `workers` forced, whatever the text size. Tests use
/// these to split short texts.
#[cfg(test)]
fn parse_condensed_in(name: &str, text: &str, workers: usize) -> Result<Vec<f64>> {
    let mut sink = CondensedSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok(sink.data)
}

#[cfg(test)]
fn parse_point_cloud_in(name: &str, text: &str, workers: usize) -> Result<Vec<Vec<f64>>> {
    let mut sink = PointSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok(sink.points)
}

#[cfg(test)]
fn parse_triplets_in(name: &str, text: &str, workers: usize) -> Result<(usize, Vec<Triplet>)> {
    let mut sink = TripletSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok((sink.n, sink.triplets))
}

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("holos_io_test_{}_{name}", std::process::id()));
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

#[test]
fn point_cloud_mixed_separators() {
    let f = TempFile::new("pc_mixed.csv", "0.0, 1.0\n2.0\t3.0\n4.0 5.0\n");
    let points = read_point_cloud(f.path(), 1).unwrap();
    assert_eq!(points, vec![vec![0.0, 1.0], vec![2.0, 3.0], vec![4.0, 5.0]]);
}

#[test]
fn point_cloud_skips_comments_and_blanks() {
    let f = TempFile::new(
        "pc_comments.csv",
        "# header comment\n\n1.0 2.0\n   \n# mid comment\n3.0 4.0\n",
    );
    let points = read_point_cloud(f.path(), 1).unwrap();
    assert_eq!(points, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
}

#[test]
fn point_cloud_inconsistent_dimension_reports_line() {
    let f = TempFile::new("pc_baddim.csv", "1.0 2.0\n\n1.0 2.0 3.0\n");
    let err = read_point_cloud(f.path(), 1).unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
    assert!(msg.contains(":3:"), "missing line number: {msg}");
    assert!(msg.contains("3 coordinates"), "{msg}");
    assert!(msg.contains("expected 2"), "{msg}");
}

#[test]
fn point_cloud_bad_token_reports_line() {
    let f = TempFile::new("pc_badtok.csv", "1.0 2.0\n1.0 oops\n");
    let err = read_point_cloud(f.path(), 1).unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
    assert!(msg.contains(":2:"), "missing line number: {msg}");
    assert!(msg.contains("oops"), "{msg}");
}

#[test]
fn missing_file_is_io_error_with_path() {
    let path = std::env::temp_dir().join(format!(
        "holos_io_test_{}_does_not_exist",
        std::process::id()
    ));
    let err = read_point_cloud(&path, 1).unwrap_err();
    assert!(matches!(err, Error::Io(_)));
    assert!(err.to_string().contains("does_not_exist"));
}

#[test]
fn lower_distance_round_trip() {
    // n = 3 condensed lower triangle: d(1,0), d(2,0), d(2,1).
    let f = TempFile::new("ld_roundtrip.lower", "1.5 2.5, 3.5\n");
    let m = read_lower_distance_matrix(f.path(), 1).unwrap();
    let direct = DistanceMatrix::from_condensed(vec![1.5, 2.5, 3.5]).unwrap();
    assert_eq!(m.len(), 3);
    for i in 0..3 {
        for j in 0..3 {
            assert_eq!(m.get(i, j), direct.get(i, j));
        }
    }
    assert_eq!(m.get(1, 0), 1.5);
    assert_eq!(m.get(2, 0), 2.5);
    assert_eq!(m.get(2, 1), 3.5);
}

#[test]
fn lower_distance_skips_comments_and_spans_lines() {
    let f = TempFile::new("ld_comments.lower", "# 4 points\n1 2\n3 4\n\n5, 6\n");
    let m = read_lower_distance_matrix(f.path(), 1).unwrap();
    assert_eq!(m.len(), 4);
    assert_eq!(m.get(3, 2), 6.0);
}

#[test]
fn lower_distance_bad_length_errors() {
    let f = TempFile::new("ld_badlen.lower", "1 2\n");
    let err = read_lower_distance_matrix(f.path(), 1).unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
    assert!(msg.contains("condensed length"), "{msg}");
}

#[test]
fn lower_distance_bad_token_reports_line() {
    let f = TempFile::new("ld_badtok.lower", "1.0\n2.0 x\n");
    let err = read_lower_distance_matrix(f.path(), 1).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains(":2:"), "missing line number: {msg}");
    assert!(msg.contains('x'), "{msg}");
}

fn slow_numbers(text: &str) -> Vec<f64> {
    text.lines()
        .filter(|l| !is_skipped(l))
        .flat_map(|l| tokens(l).map(|t| t.parse::<f64>().unwrap()))
        .collect()
}

#[test]
fn scanner_matches_str_parse_bit_for_bit() {
    let text = "0.5098973069436853, 1e-320 -0.0\n+3.25e2\t.5 5. inf -Infinity NaN\n\
                17976931348623157e292 2.2250738585072014e-308 4.9e-324 1e400\n";
    let fast = parse_condensed("t", text, 1).unwrap();
    let slow = slow_numbers(text);
    assert_eq!(fast.len(), slow.len());
    for (a, b) in fast.iter().zip(&slow) {
        assert!(
            a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
            "{a} vs {b}"
        );
    }
}

#[test]
fn scanner_rejects_what_str_parse_rejects() {
    for bad in [
        "1e", "1_000", "0x10", "1.2.3", "--1", "1e5x", "-", "+", "nanx", "1,,x",
    ] {
        let text = format!("1.0\n{bad}\n");
        let err = parse_condensed("t", &text, 1).unwrap_err().to_string();
        assert!(err.contains("t:2: not a number"), "{bad}: {err}");
    }
}

#[test]
fn scanner_falls_back_on_unicode_whitespace() {
    // U+00A0 is whitespace to `char::is_whitespace`, so both paths split on it.
    let text = "1.0\u{a0}2.0\n3.0\n";
    assert_eq!(parse_condensed("t", text, 1).unwrap(), vec![1.0, 2.0, 3.0]);
    let (n, t) = parse_triplets("t", "0\u{a0}1 0.5\n", 1).unwrap();
    assert_eq!((n, t), (2, vec![(0, 1, 0.5)]));
}

#[test]
fn triplet_scanner_accepts_the_slow_grammar() {
    let (n, t) = parse_triplets("t", "# c\n\n 3,1 0.25\r\n+2 0 1e-3\n", 1).unwrap();
    assert_eq!(n, 4);
    assert_eq!(t, vec![(3, 1, 0.25), (2, 0, 1e-3)]);
    for (bad, msg) in [
        ("1 2", "expected 'i j d', got 2 fields"),
        ("1 2 3 4", "expected 'i j d', got 4 fields"),
        ("-1 2 0.5", "not a vertex index: \"-1\""),
        ("1 x 0.5", "not a vertex index: \"x\""),
        ("1 2 abc", "not a number: \"abc\""),
        ("99999999999999999999999 2 0.5", "not a vertex index"),
    ] {
        let err = parse_triplets("t", bad, 1).unwrap_err().to_string();
        assert!(err.contains(&format!("t:1: {msg}")), "{bad}: {err}");
    }
}

proptest::proptest! {
    #[test]
    fn scanner_matches_str_parse_on_random_text(
        values in proptest::collection::vec(proptest::num::f64::ANY, 0..40),
        seps in proptest::collection::vec(0u8..4, 0..40),
    ) {
        let mut text = String::new();
        for (i, v) in values.iter().enumerate() {
            let sep = match seps.get(i).copied().unwrap_or(0) { 0 => " ", 1 => ",", 2 => "\n", _ => "\t," };
            if i % 3 == 0 { text.push_str(&format!("{v:e}")); } else { text.push_str(&v.to_string()); }
            text.push_str(sep);
        }
        let fast = parse_condensed("t", &text, 1).unwrap();
        let slow = slow_numbers(&text);
        proptest::prop_assert_eq!(fast.len(), slow.len());
        for (a, b) in fast.iter().zip(&slow) {
            proptest::prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
        }
    }
}

/// Forty lines of two coordinates each, with `bad` replaced by `line`.
fn lines_with(bad: usize, line: &str) -> String {
    let mut text = String::new();
    for lineno in 1..=40 {
        if lineno == bad {
            text.push_str(line);
        } else {
            text.push_str("1.0 2.0");
        }
        text.push('\n');
    }
    text
}

#[test]
fn chunked_condensed_reports_the_serial_error() {
    // Line 1 lands in the first chunk of four, line 20 in the middle,
    // line 40 in the last.
    for bad in [1, 20, 40] {
        let text = lines_with(bad, "1.0 oops");
        let serial = parse_condensed_in("t", &text, 1).unwrap_err().to_string();
        let chunked = parse_condensed_in("t", &text, 4).unwrap_err().to_string();
        assert_eq!(serial, chunked);
        assert!(
            serial.contains(&format!("t:{bad}: not a number")),
            "{serial}"
        );
    }
}

#[test]
fn chunked_condensed_reports_the_first_error_in_file_order() {
    let mut text = String::new();
    for lineno in 1..=40 {
        match lineno {
            12 => text.push_str("1.0 first\n"),
            31 => text.push_str("1.0 second\n"),
            _ => text.push_str("1.0 2.0\n"),
        }
    }
    for workers in [1, 2, 4, 8] {
        let err = parse_condensed_in("t", &text, workers)
            .unwrap_err()
            .to_string();
        assert_eq!(
            err, "invalid input: t:12: not a number: \"first\"",
            "{workers}"
        );
    }
}

#[test]
fn chunked_condensed_accepts_the_serial_grammar() {
    // CRLF, a comment, blank and separator-only lines, a line the byte
    // scanner hands to the Unicode fallback, and no final newline.
    let text = "# comment\r\n1.0, 2.0\r\n\r\n3.0\u{a0}4.0\n   \n\t,\n5.0 6.0";
    let serial = parse_condensed_in("t", text, 1).unwrap();
    assert_eq!(serial, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(parse_condensed_in("t", text, 4).unwrap(), serial);
}

#[test]
fn chunked_point_cloud_accepts_the_serial_grammar() {
    let text = "# comment\r\n1.0, 2.0\r\n\r\n3.0\u{a0}4.0\n   \n\t,\n5.0 6.0";
    let serial = parse_point_cloud_in("t", text, 1).unwrap();
    assert_eq!(serial, vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    assert_eq!(parse_point_cloud_in("t", text, 4).unwrap(), serial);
}

#[test]
fn chunked_point_cloud_reports_the_serial_width_error() {
    // The wrong width can precede a bad token inside one chunk, and it
    // can be the chunk's first point, which no chunk-local comparison
    // sees. Sweep both lines over every position.
    for wide in 1..=40 {
        let mut text = String::new();
        for lineno in 1..=40 {
            if lineno == wide {
                text.push_str("1.0 2.0 3.0\n");
            } else if lineno == wide + 4 {
                text.push_str("1.0 oops\n");
            } else {
                text.push_str("1.0 2.0\n");
            }
        }
        let serial = parse_point_cloud_in("t", &text, 1);
        for workers in [2, 4, 8] {
            let chunked = parse_point_cloud_in("t", &text, workers);
            match (&serial, &chunked) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "wide {wide}, workers {workers}"),
                (Err(a), Err(b)) => {
                    assert_eq!(
                        a.to_string(),
                        b.to_string(),
                        "wide {wide}, workers {workers}"
                    )
                }
                _ => panic!("wide {wide}, workers {workers}: paths disagree"),
            }
        }
    }
}

#[test]
fn chunked_triplets_match_the_serial_parse() {
    let mut text = String::from("# comment\n\n");
    for i in 0..40u32 {
        text.push_str(&format!("{i} {} 0.{i}\r\n", i + 1));
    }
    text.push_str("0\u{a0}7 0.5\n41 3 0.25");
    let serial = parse_triplets_in("t", &text, 1).unwrap();
    assert_eq!(serial.0, 42);
    for workers in [2, 4, 8] {
        assert_eq!(parse_triplets_in("t", &text, workers).unwrap(), serial);
    }
}

#[test]
fn chunked_triplets_report_the_serial_error() {
    for bad in [1, 20, 40] {
        let mut text = String::new();
        for lineno in 1..=40u32 {
            if lineno == bad {
                text.push_str("3 x 0.5\n");
            } else {
                text.push_str("1 2 0.5\n");
            }
        }
        let serial = parse_triplets_in("t", &text, 1).unwrap_err().to_string();
        assert_eq!(
            parse_triplets_in("t", &text, 4).unwrap_err().to_string(),
            serial
        );
        assert!(
            serial.contains(&format!("t:{bad}: not a vertex index")),
            "{serial}"
        );
    }
}

#[test]
fn the_public_parse_splits_a_text_over_the_threshold() {
    let mut text = String::with_capacity(MIN_PARALLEL_BYTES + 64);
    while text.len() < MIN_PARALLEL_BYTES {
        text.push_str("0.5098973069436853 1.25e-3 -7\n");
    }
    assert_eq!(parse_workers(&text, 4, MIN_PARALLEL_BYTES), 4);
    let serial = parse_condensed("t", &text, 1).unwrap();
    for threads in [2, 4, 8] {
        let parallel = parse_condensed("t", &text, threads).unwrap();
        assert_eq!(serial.len(), parallel.len());
        assert!(
            serial
                .iter()
                .zip(&parallel)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{threads} workers"
        );
    }
    text.push_str("1.0 nope\n");
    assert_eq!(
        parse_condensed("t", &text, 4).unwrap_err().to_string(),
        parse_condensed("t", &text, 1).unwrap_err().to_string()
    );
}

#[test]
fn a_small_text_stays_serial() {
    assert_eq!(parse_workers("1.0 2.0\n", 8, MIN_PARALLEL_BYTES), 1);
    assert_eq!(parse_workers("1.0 2.0\n", 1, MIN_PARALLEL_BYTES), 1);
}

#[test]
fn line_chunks_cover_the_text_and_end_at_newlines() {
    for text in ["", "\n", "a\nb\n", "a\nb", "abc", "a\n\n\nb\n"] {
        for workers in [2, 3, 8] {
            let chunks = line_chunks(text, workers);
            assert_eq!(chunks.concat(), text, "{text:?} {workers}");
            let last = chunks.iter().rposition(|chunk| !chunk.is_empty());
            for (k, chunk) in chunks.iter().enumerate() {
                assert!(
                    chunk.is_empty() || chunk.ends_with('\n') || Some(k) == last,
                    "{text:?} {workers}: {chunk:?}"
                );
            }
            let split: Vec<&str> = chunks.iter().flat_map(|c| c.lines()).collect();
            let whole: Vec<&str> = text.lines().collect();
            assert_eq!(split, whole, "{text:?} {workers}");
        }
    }
}

proptest::proptest! {
    #[test]
    fn chunked_parse_matches_serial_on_random_text(
        values in proptest::collection::vec(proptest::num::f64::ANY, 0..40),
        seps in proptest::collection::vec(0u8..5, 0..40),
        kinds in proptest::collection::vec(0u8..8, 0..40),
    ) {
        let mut text = String::new();
        for (i, v) in values.iter().enumerate() {
            match kinds.get(i).copied().unwrap_or(3) {
                0 => text.push_str("# comment"),
                1 => text.push_str("oops"),
                2 => text.push_str(&format!("{v:e}")),
                _ => text.push_str(&v.to_string()),
            }
            text.push_str(match seps.get(i).copied().unwrap_or(0) {
                0 => " ",
                1 => ",",
                2 => "\n",
                3 => "\r\n",
                _ => "\t",
            });
        }
        match (parse_condensed_in("t", &text, 1), parse_condensed_in("t", &text, 3)) {
            (Ok(serial), Ok(chunked)) => {
                proptest::prop_assert_eq!(serial.len(), chunked.len());
                for (a, b) in serial.iter().zip(&chunked) {
                    proptest::prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
                }
            }
            (Err(serial), Err(chunked)) => {
                proptest::prop_assert_eq!(serial.to_string(), chunked.to_string());
            }
            _ => proptest::prop_assert!(false, "one path failed and the other did not"),
        }
    }
}

#[test]
fn newline_count_matches_the_scalar_count() {
    let mut text = Vec::new();
    for i in 0..1000u32 {
        text.push(if i % 3 == 0 { b'\n' } else { (i % 251) as u8 });
    }
    for end in [0, 1, 7, 8, 9, 15, 16, 17, 100, 999, 1000] {
        let slice = &text[..end];
        assert_eq!(
            count_newlines(slice),
            slice.iter().filter(|&&b| b == b'\n').count()
        );
    }
    assert_eq!(count_newlines(&[0x0b; 16]), 0);
    assert_eq!(count_newlines(&[0x8a; 16]), 0);
    assert_eq!(count_newlines(&[0x0a; 16]), 16);
}
