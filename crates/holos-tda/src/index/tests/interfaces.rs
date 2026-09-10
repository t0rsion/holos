use super::support::*;
use super::*;

#[test]
fn zero_cone_interface_is_dimension_generic_and_checked_by_fallback() {
    let graph = zero_cone_cover(false);
    let index_params = IndexParams {
        max_separator_width: 3,
        leaf_vertices: 4,
        ..compose_index_params()
    };
    for max_dim in 0..=3 {
        let params = RipsParams::new(max_dim).with_modulus(3);
        let composed =
            PersistenceIndex::compile(&graph, &params, index_params, CertificateLimits::default())
                .unwrap();
        assert_eq!(composed.interfaces()[0].mode, InterfaceMode::ZeroCone);
        assert_eq!(
            composed.diagram().bars,
            rips_persistence_sparse(&graph, &params).unwrap().bars
        );
        let materialized = PersistenceIndex::compile(
            &graph,
            &params,
            IndexParams {
                interface_policy: InterfacePolicy::Materialize,
                ..index_params
            },
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(composed.diagram().bars, materialized.diagram().bars);
    }

    let params = RipsParams::new(2).with_modulus(3);
    let nonzero = PersistenceIndex::compile(
        &zero_cone_cover(true),
        &params,
        index_params,
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(nonzero.interfaces()[0].mode, InterfaceMode::Materialized);

    let composed =
        PersistenceIndex::compile(&graph, &params, index_params, CertificateLimits::default())
            .unwrap();
    let materialized = composed.transition(&zero_cone_cover(true)).unwrap();
    assert_eq!(
        materialized.index.interfaces()[0].mode,
        InterfaceMode::Materialized
    );
    assert_eq!(materialized.mode, IndexUpdateMode::Rebuilt);
    let restored = materialized.index.transition(&graph).unwrap();
    assert_eq!(restored.index.interfaces()[0].mode, InterfaceMode::ZeroCone);
    assert!(restored.work.nodes_composed > 0);
}

#[test]
fn h2_composes_across_a_zero_vertex_and_repairs_one_route() {
    let initial = joined_octahedra(false);
    let updated = joined_octahedra(true);
    let index_params = IndexParams {
        max_separator_width: 1,
        leaf_vertices: 6,
        ..compose_index_params()
    };
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(2).with_modulus(modulus);
        let first = PersistenceIndex::compile(
            &initial,
            &params,
            index_params,
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(first.summary().root_composed);
        assert_eq!(first.diagram().in_dim(2).count(), 2);
        assert_eq!(
            first.diagram().bars,
            rips_persistence_sparse(&initial, &params).unwrap().bars
        );
        let transition = first.transition(&updated).unwrap();
        assert!(transition.work.nodes_shared > 0);
        assert!(transition.work.nodes_composed > 0);
        assert_eq!(
            transition.index.diagram().bars,
            rips_persistence_sparse(&updated, &params).unwrap().bars
        );
    }
}

#[test]
fn nonzero_separator_has_an_exact_root_interface() {
    let graph = shared_edge_graph(0.25);
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let index = PersistenceIndex::compile(
            &graph,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let expected = rips_persistence_sparse(&graph, &params).unwrap();
        assert_eq!(index.diagram().bars, expected.bars);
        assert!(index.summary().separators > 0);
        assert!(!index.summary().root_composed);
        assert!(
            index
                .interfaces()
                .iter()
                .any(|interface| interface.separator == [0, 1])
        );
    }
}

#[test]
fn zero_simplex_separator_omits_the_global_reduction() {
    let graph = shared_edge_graph(0.0);
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let index = PersistenceIndex::compile(
            &graph,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(
            index.diagram().bars,
            rips_persistence_sparse(&graph, &params).unwrap().bars
        );
        assert!(index.summary().root_composed);
        assert!(index.summary().composed_interfaces > 0);
        assert_eq!(index.interfaces()[0].mode, InterfaceMode::ZeroSimplex);
        assert_eq!(index.interfaces()[0].reduction_columns, 0);

        let baseline_params = IndexParams {
            interface_policy: InterfacePolicy::Materialize,
            ..IndexParams::default()
        };
        let baseline = PersistenceIndex::compile(
            &graph,
            &params,
            baseline_params,
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(baseline.diagram().bars, index.diagram().bars);
        assert!(!baseline.summary().root_composed);
        assert_eq!(baseline.summary().composed_interfaces, 0);
    }
}

#[test]
fn a_separator_edit_switches_between_composed_and_materialized_states() {
    let initial = shared_edge_graph(0.0);
    let updated = shared_edge_graph(0.25);
    let params = RipsParams::new(1).with_modulus(3);
    let first = PersistenceIndex::compile(
        &initial,
        &params,
        compose_index_params(),
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&updated).unwrap();
    assert!(first.summary().root_composed);
    assert!(!second.index.summary().root_composed);
    assert_eq!(second.mode, IndexUpdateMode::Rebuilt);
    assert_eq!(
        second.index.diagram().bars,
        rips_persistence_sparse(&updated, &params).unwrap().bars
    );
    let third = second.index.transition(&initial).unwrap();
    assert!(third.index.summary().root_composed);
    assert!(third.work.nodes_composed > 0);
    assert_eq!(
        third.index.diagram().bars,
        rips_persistence_sparse(&initial, &params).unwrap().bars
    );
}
