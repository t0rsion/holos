//! Optional artifacts for independent checker measurements.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use holos_tda::SparseDistanceMatrix;

pub(crate) fn write(
    prefix: &Path,
    graph: &SparseDistanceMatrix,
    program: &[u8],
    trace: &[u8],
) -> Result<(), String> {
    let graph_path = suffixed(prefix, ".graph");
    let program_path = suffixed(prefix, ".program");
    let trace_path = suffixed(prefix, ".trace");
    if let Some(parent) = graph_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create artifact directory: {error}"))?;
    }
    fs::write(&graph_path, graph_text(graph))
        .map_err(|error| format!("cannot write source graph: {error}"))?;
    fs::write(&program_path, program)
        .map_err(|error| format!("cannot write program artifact: {error}"))?;
    fs::write(&trace_path, trace)
        .map_err(|error| format!("cannot write trace artifact: {error}"))?;
    Ok(())
}

fn graph_text(graph: &SparseDistanceMatrix) -> String {
    let mut output = format!("{}\n", graph.len());
    for (u, v, value) in graph.edges() {
        writeln!(output, "{u} {v} {value}").expect("writing to a string cannot fail");
    }
    output
}

fn suffixed(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path = prefix.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_round_trippable_graph_and_artifact_bytes() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let prefix = std::env::temp_dir().join(format!(
            "holos-program-bench-{}-{stamp}",
            std::process::id()
        ));
        let graph = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 0.5), (1, 2, 1.25)]).unwrap();
        write(&prefix, &graph, b"program", b"trace").unwrap();
        let graph_text = fs::read_to_string(suffixed(&prefix, ".graph")).unwrap();
        assert_eq!(graph_text, "3\n0 1 0.5\n1 2 1.25\n");
        assert_eq!(fs::read(suffixed(&prefix, ".program")).unwrap(), b"program");
        assert_eq!(fs::read(suffixed(&prefix, ".trace")).unwrap(), b"trace");
        for suffix in [".graph", ".program", ".trace"] {
            fs::remove_file(suffixed(&prefix, suffix)).unwrap();
        }
    }
}
