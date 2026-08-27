fn candidates(
    matrix: &[Vec<f64>],
    u: usize,
    v: usize,
    edge_value: f64,
    terminal: f64,
) -> Vec<(usize, f64)> {
    matrix[u]
        .iter()
        .zip(&matrix[v])
        .enumerate()
        .filter_map(|(vertex, (&left, &right))| {
            if vertex == u || vertex == v || !left.is_finite() || !right.is_finite() {
                return None;
            }
            let birth = edge_value.max(left).max(right);
            (birth <= terminal).then_some((vertex, birth))
        })
        .collect()
}

fn critical_values(edge_value: f64, candidates: &[(usize, f64)]) -> Vec<f64> {
    let mut values: Vec<_> = std::iter::once(edge_value)
        .chain(candidates.iter().map(|&(_, birth)| birth))
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    values
}

fn level_vertices(candidates: &[(usize, f64)], value: f64) -> Vec<usize> {
    candidates
        .iter()
        .filter(|&&(_, birth)| birth <= value)
        .map(|&(vertex, _)| vertex)
        .collect()
}

fn dominates(matrix: &[Vec<f64>], level: &[usize], apex: usize, value: f64) -> bool {
    level
        .iter()
        .all(|&vertex| vertex == apex || matrix[apex][vertex] <= value)
}

fn remains_apex(
    matrix: &[Vec<f64>],
    candidates: &[(usize, f64)],
    level: &[usize],
    apex: usize,
    value: f64,
) -> bool {
    let birth = candidates
        .iter()
        .find(|&&(vertex, _)| vertex == apex)
        .map(|&(_, birth)| birth);
    birth.is_some_and(|birth| birth <= value) && dominates(matrix, level, apex, value)
}

pub fn ref_test_edge(
    matrix: &[Vec<f64>],
    u: usize,
    v: usize,
    edge_value: f64,
    terminal: f64,
) -> Option<Vec<(f64, usize)>> {
    let candidates = candidates(matrix, u, v, edge_value, terminal);
    let mut segments = Vec::new();
    let mut apex = None;
    for value in critical_values(edge_value, &candidates) {
        let level = level_vertices(&candidates, value);
        if level.is_empty() {
            return None;
        }
        if apex.is_some_and(|current| remains_apex(matrix, &candidates, &level, current, value)) {
            continue;
        }
        let next = level
            .iter()
            .copied()
            .find(|&candidate| dominates(matrix, &level, candidate, value))?;
        segments.push((value, next));
        apex = Some(next);
    }
    Some(segments)
}
