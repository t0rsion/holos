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
    BipersistenceProofLimits, CircularProofLimits, IndexProofState, ProofLimits, is_bipersistence,
    is_circular_coordinate, is_cohomology_intervention, is_coverage, is_geometry_bound_coverage,
    is_kinetic_zigzag, is_relative_interface, is_synthesis, verify_bipersistence,
    verify_circular_coordinate, verify_cohomology_intervention, verify_coverage,
    verify_distributed_interface, verify_geometry_bound_coverage, verify_kinetic_zigzag,
    verify_relative_interface, verify_synthesis,
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

mod artifacts;
mod basic;
mod cohomology;
mod collapse;
mod coverage;
mod engine;
mod errors;
mod index;
