use super::*;

/// The window reader against the one-window parse, at a window of 64
/// bytes so a small file crosses many windows: a line longer than the
/// window, a missing final newline, and a bad token in a late window.
#[test]
fn windows_give_the_one_window_parse() {
    let mut text = String::new();
    for i in 0..400 {
        text.push_str(&format!("{}.5 {}e-3\n", i, i * 7));
    }
    text.push_str(&"1.0 ".repeat(100));
    text.push_str("\n# comment\n\n2.0 3.0");
    let f = TempFile::new("win_ok.lower", &text);
    let file = std::fs::File::open(f.path()).unwrap();
    let mut windows = Windows::new(file, 64);
    let mut joined = Vec::new();
    let mut buf = Vec::new();
    let mut count = 0;
    while windows.next_into(&mut buf).unwrap() {
        assert!(buf.ends_with(b"\n") || joined.len() + buf.len() == text.len());
        joined.extend_from_slice(&buf);
        count += 1;
    }
    assert_eq!(joined, text.as_bytes());
    assert!(count > 10, "{count} windows");
    for threads in [1, 3] {
        let mut sink = CondensedSink::new(threads);
        let mut windows = Windows::new(std::fs::File::open(f.path()).unwrap(), 64);
        let mut first_line = 1;
        let mut buf = Vec::new();
        while windows.next_into(&mut buf).unwrap() {
            let chunk = std::str::from_utf8(&buf).unwrap();
            sink.window("t", first_line, chunk).unwrap();
            first_line += buf.iter().filter(|&&b| b == b'\n').count();
        }
        assert_eq!(sink.data, parse_condensed("t", &text, 1).unwrap());
    }
    let bad = format!("{text}\n7 8 x\n");
    let f = TempFile::new("win_bad.lower", &bad);
    for threads in [1, 4] {
        let err = read_lower_distance_matrix(f.path(), threads)
            .unwrap_err()
            .to_string();
        let serial = parse_condensed("t", &bad, 1).unwrap_err().to_string();
        let expected = ":405: not a number: \"x\"";
        assert!(err.ends_with(expected), "{err}");
        assert!(serial.ends_with(expected), "{serial}");
    }
}
