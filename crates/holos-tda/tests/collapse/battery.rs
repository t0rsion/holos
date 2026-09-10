use super::fixtures::*;

#[test]
fn collapse_preserves_the_diagram_on_a_point_cloud() {
    assert_collapse_preserves_diagram("points", &battery_points(), 0.7, true);
}

#[test]
fn collapse_preserves_the_diagram_with_ties() {
    assert_collapse_preserves_diagram("ties", &battery_ties(), 1.0, true);
}

#[test]
fn collapse_preserves_the_diagram_with_zero_distances() {
    assert_collapse_preserves_diagram("zeros", &battery_zeros(), 0.6, true);
}

#[test]
fn collapse_preserves_the_diagram_with_absent_edges() {
    assert_collapse_preserves_diagram("infinite", &battery_infinite(), 2.0, true);
}

#[test]
fn collapse_preserves_the_diagram_when_disconnected() {
    assert_collapse_preserves_diagram("disconnected", &battery_disconnected(), 1.5, true);
}

#[test]
fn certificate_properties_hold_on_every_battery_input() {
    let cases: [(&str, DistanceMatrix, f64); 5] = [
        ("points", battery_points(), 0.7),
        ("ties", battery_ties(), 1.0),
        ("zeros", battery_zeros(), 0.6),
        ("infinite", battery_infinite(), 2.0),
        ("disconnected", battery_disconnected(), 1.5),
    ];
    for (name, dense, mid) in cases {
        let sparse = sparse_from_dense(&dense);
        for threshold in [None, Some(mid), Some(f64::INFINITY)] {
            let label = format!("{name} threshold={threshold:?}");
            collapse_and_check_dense(&label, &dense, threshold);
            collapse_and_check_sparse(&label, &sparse, threshold);
        }
    }
}

#[test]
fn dense_and_sparse_forms_agree_on_one_graph() {
    // Same graph, same explicit threshold: the two entry points must produce
    // the same schedule, the same witnesses, and the same output matrix.
    let dense = level_dependent_apex_matrix();
    let sparse = sparse_from_dense(&dense);
    let threshold = Some(2.0);
    let from_dense = collapse_dense(&dense, threshold).unwrap();
    let from_sparse = collapse_sparse(&sparse, threshold).unwrap();
    assert_eq!(
        from_dense.certificate, from_sparse.certificate,
        "dense and sparse certificates differ"
    );
    assert_eq!(
        edge_list(&from_dense.matrix),
        edge_list(&from_sparse.matrix),
        "dense and sparse output matrices differ"
    );
    assert_eq!(
        from_dense.stats, from_sparse.stats,
        "dense and sparse stats differ"
    );
}
