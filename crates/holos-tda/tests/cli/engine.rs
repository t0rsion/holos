use super::*;

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
