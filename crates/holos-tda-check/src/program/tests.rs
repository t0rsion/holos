use super::graph::ProgramGraph;
use super::model::ProgramProofLimits;
use super::verify::program_digest;
use super::{is_program, verify_program};

fn empty_program() -> (Vec<u8>, ProgramGraph) {
    let graph = ProgramGraph::from_triplets(2, &[]).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"HOLOSPRG");
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&2u64.to_be_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&2u64.to_be_bytes());
    bytes.extend_from_slice(&0u64.to_be_bytes());
    bytes.extend_from_slice(&program_digest(&graph, None));
    for _ in 0..2 {
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&f64::INFINITY.to_bits().to_be_bytes());
    }
    (bytes, graph)
}

#[test]
fn program_without_cycles_verifies_against_its_source_graph() {
    let (bytes, graph) = empty_program();
    assert!(is_program(&bytes));
    let checked = verify_program(&bytes, &graph, ProgramProofLimits::default()).unwrap();
    assert_eq!(checked.vertices, 2);
    assert_eq!(checked.edges, 0);
    assert_eq!(checked.atoms, 0);
    assert_eq!(checked.h0_bars, 2);
    assert_eq!(checked.h1_bars, 0);
}

#[test]
fn program_mutation_and_truncation_are_rejected() {
    let (bytes, graph) = empty_program();
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    assert!(verify_program(&changed, &graph, ProgramProofLimits::default()).is_err());
    for end in 0..bytes.len() {
        assert!(verify_program(&bytes[..end], &graph, ProgramProofLimits::default()).is_err());
    }
}

#[test]
fn arbitrary_program_bytes_do_not_panic() {
    for length in 0..4096 {
        let bytes: Vec<u8> = (0..length)
            .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
            .collect();
        let _ = verify_program(
            &bytes,
            &ProgramGraph::from_triplets(0, &[]).unwrap(),
            ProgramProofLimits::default(),
        );
    }
}

#[test]
fn source_graph_parser_accepts_sparse_triplets_and_preserves_isolates() {
    let graph = ProgramGraph::parse_text(
        b"# vertex_count=5 is explicit\n2, 0, 1.5\n1 2 2.0 # inline comment\n",
    )
    .unwrap();
    assert_eq!(graph.vertex_count(), 5);
    assert_eq!(graph.num_edges(), 2);
    assert_eq!(graph.edges()[0], (0, 2, 1.5).into());
    assert_eq!(graph.edges()[1], (1, 2, 2.0).into());

    let inferred = ProgramGraph::parse_text(b"0 3 4\n").unwrap();
    assert_eq!(inferred.vertex_count(), 4);
    assert_eq!(inferred.get(3, 0), 4.0);

    let explicit = ProgramGraph::parse_text(b"5\n0 1 1\n").unwrap();
    assert_eq!(explicit.vertex_count(), 5);
    assert_eq!(explicit.num_edges(), 1);

    let edges = [super::super::ProgramEdge::new(1, 2, 3.0)];
    let borrowed = ProgramGraph::from_edges(3, edges.iter()).unwrap();
    assert_eq!(borrowed.edges(), &edges);
}
