use std::fs;

use holos_tda::{DistanceMatrix, SparseDistanceMatrix};

use super::args::{Args, file_stem};

pub(super) struct StudyInput {
    pub(super) points: Vec<Vec<f64>>,
    pub(super) graph: SparseDistanceMatrix,
    pub(super) triangles: u64,
    pub(super) tetrahedra: u64,
}

pub(super) fn prepare_study_input(args: &Args) -> Result<StudyInput, String> {
    let points = read_cloud(&args.input)?;
    if points.len() < 2 {
        return Err(format!("{}: need at least two points", args.input));
    }

    let study_graph = threshold_to_sparse(
        &DistanceMatrix::from_points(&points).map_err(|error| error.to_string())?,
        args.threshold,
    )?;
    let (triangles, tetrahedra) = graph_cliques(&study_graph);
    Ok(StudyInput {
        points,
        graph: study_graph,
        triangles,
        tetrahedra,
    })
}

pub(super) fn graph_cliques(graph: &SparseDistanceMatrix) -> (u64, u64) {
    let mut adjacency = vec![Vec::new(); graph.len()];
    for (u, v, _) in graph.edges() {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    let mut triangles = 0u64;
    let mut tetrahedra = 0u64;
    for u in 0..graph.len() {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            let common = sorted_intersection_above(&adjacency[u], &adjacency[v], v);
            triangles = triangles.saturating_add(common.len() as u64);
            for (position, &w) in common.iter().enumerate() {
                for &x in &common[position + 1..] {
                    if adjacency[w].binary_search(&x).is_ok() {
                        tetrahedra = tetrahedra.saturating_add(1);
                    }
                }
            }
        }
    }
    (triangles, tetrahedra)
}

fn sorted_intersection_above(a: &[usize], b: &[usize], lower: usize) -> Vec<usize> {
    let mut intersection = Vec::new();
    let mut i = a.partition_point(|&value| value <= lower);
    let mut j = b.partition_point(|&value| value <= lower);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                intersection.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    intersection
}

pub(super) fn threshold_to_sparse(
    dense: &DistanceMatrix,
    threshold: f64,
) -> Result<SparseDistanceMatrix, String> {
    let mut edges = Vec::new();
    for u in 0..dense.len() {
        for v in u + 1..dense.len() {
            let value = dense.get(u, v);
            if value.is_finite() && value <= threshold {
                edges.push((u, v, value));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(dense.len(), &edges).map_err(|error| error.to_string())
}

fn read_cloud(path: &str) -> Result<Vec<Vec<f64>>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", file_stem(path)))?;
    let mut points = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut point = Vec::new();
        for token in line.replace(',', " ").split_whitespace() {
            point.push(token.parse::<f64>().map_err(|_| {
                format!(
                    "{} line {}: {token} is not a number",
                    file_stem(path),
                    line_index + 1
                )
            })?);
        }
        points.push(point);
    }
    Ok(points)
}

pub(super) fn vm_hwm_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}
