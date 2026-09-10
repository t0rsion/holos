use super::*;

#[test]
fn v2_is_thread_invariant() {
    assert_thread_invariant("tie_heavy_grid", &tie_heavy_grid(), None);
    assert_thread_invariant("tie_heavy_grid t=3", &tie_heavy_grid(), Some(3.0));
    assert_thread_invariant("random16", &random_matrix(0x7a1e_0001, 16, 0.6), Some(2.0));
    assert_thread_invariant("random24", &random_matrix(0x7a1e_0002, 24, 0.4), None);
    assert_thread_invariant("k5", &complete_matrix(5), Some(1.0));
    assert_thread_invariant("k4x8", &disjoint_k4_matrix(8), Some(1.0));
    assert_thread_invariant("fallback", &fallback_matrix(), Some(1.0));
    assert_thread_invariant("k64_64+k4", &bipartite_k4_dense(), None);
}

#[test]
fn v2_preserves_the_diagram() {
    assert_v2_preserves_the_diagram("points", &battery_points(), 0.7, true);
    assert_v2_preserves_the_diagram("ties", &battery_ties(), 1.0, true);
    assert_v2_preserves_the_diagram("diamond", &diamond_matrix(), 1.0, true);
}
