use super::args::parse_args;
use super::input::graph_cliques;
use super::reporting::summarize;
use holos_tda::SparseDistanceMatrix;

#[test]
fn config_parser_preserves_order_and_removes_duplicates() {
    let argv = [
        "--input",
        "cloud.csv",
        "--threshold",
        "1",
        "--configs",
        "v3-h2,none,v3-h2,v1",
    ]
    .map(String::from);
    let args = parse_args(&argv).unwrap();
    assert_eq!(
        args.kinds
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>(),
        ["v3-h2", "none", "v1"]
    );
}

#[test]
fn summary_uses_interpolated_quartiles() {
    let mut values = [1.0, 4.0, 2.0, 3.0];
    let summary = summarize(&mut values);
    assert_eq!(summary.median, 2.5);
    assert_eq!(summary.iqr, 1.5);
    assert_eq!(summary.max, 4.0);
}

#[test]
fn clique_counter_counts_each_simplex_once() {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (0, 3, 1.0),
        (1, 2, 1.0),
        (1, 3, 1.0),
        (2, 3, 1.0),
    ];
    let graph = SparseDistanceMatrix::from_triplets(4, &edges).unwrap();
    assert_eq!(graph_cliques(&graph), (4, 1));
}
