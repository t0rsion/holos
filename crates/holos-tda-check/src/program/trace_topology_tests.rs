use super::super::graph::{ProgramEdge, ProgramGraph};
use super::{TraceEventKind, program_blocks, repair_event_kind, separator_edges};

#[test]
fn class_space_rebuild_is_not_reported_as_suffix_repair() {
    assert_eq!(
        repair_event_kind(8, 0),
        TraceEventKind::AtomRebuilt,
        "retaining every reduction column still rebuilds a failed class-space state"
    );
    assert_eq!(
        repair_event_kind(0, 8),
        TraceEventKind::AtomRebuilt,
        "reducing every column rebuilds the atom"
    );
    assert_eq!(
        repair_event_kind(8, 2),
        TraceEventKind::ReductionSuffixRepaired
    );
}

#[test]
fn separator_edges_include_an_acyclic_sibling() {
    let graph = ProgramGraph::from_edges(
        6,
        [
            ProgramEdge::new(0, 1, 0.0),
            ProgramEdge::new(0, 2, 1.0),
            ProgramEdge::new(1, 2, 1.0),
            ProgramEdge::new(0, 3, 1.0),
            ProgramEdge::new(1, 4, 1.0),
            ProgramEdge::new(3, 4, 1.0),
            ProgramEdge::new(4, 5, 1.0),
            ProgramEdge::new(3, 5, 1.0),
        ],
    )
    .unwrap();
    let blocks = program_blocks(&graph.proof_graph(), None).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].vertices, vec![0, 1, 2]);
    assert_eq!(blocks[1].vertices, vec![0, 1, 3, 4, 5]);
    assert_eq!(separator_edges(&graph, None).unwrap(), vec![[0, 1]]);
}
