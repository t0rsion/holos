use holos_tda::SparseDistanceMatrix;

pub(crate) fn complete_edges(vertex_count: usize) -> Vec<(usize, usize)> {
    (0..vertex_count)
        .flat_map(|right| (0..right).map(move |left| (left, right)))
        .collect()
}

pub(crate) fn for_each_graph_state(
    vertex_count: usize,
    mut visit: impl FnMut(&SparseDistanceMatrix),
) {
    let possible = complete_edges(vertex_count);
    let states = 4usize.pow(possible.len() as u32);
    for mut state in 0..states {
        let mut topology = Vec::new();
        let mut weights = Vec::new();
        for &edge in &possible {
            let edge_state = state % 4;
            state /= 4;
            if edge_state > 0 {
                topology.push(edge);
                weights.push((edge_state - 1) as f64);
            }
        }
        visit(&graph(vertex_count, &topology, &weights));
    }
}

pub(crate) fn for_each_weighting(count: usize, mut visit: impl FnMut(&[f64])) {
    let assignments = 3usize.pow(count as u32);
    let mut weights = vec![0.0; count];
    for mut assignment in 0..assignments {
        for weight in &mut weights {
            *weight = (assignment % 3) as f64;
            assignment /= 3;
        }
        visit(&weights);
    }
}

pub(crate) fn graph(
    vertex_count: usize,
    endpoints: &[(usize, usize)],
    weights: &[f64],
) -> SparseDistanceMatrix {
    let triplets = endpoints
        .iter()
        .copied()
        .zip(weights.iter().copied())
        .map(|((u, v), weight)| (u, v, weight))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(vertex_count, &triplets).unwrap()
}
