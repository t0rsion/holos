use super::wire::{F64_BITS_CODEC, SNAPSHOT_MAGIC, VERSION, decode_delta, decode_snapshot};
use super::wire_reader::Reader;
use crate::ProofLimits;

fn put_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn snapshot_prefix(vertex_count: u64, edge_count: u64, node_count: u64) -> Vec<u8> {
    let mut bytes = header(vertex_count);
    put_u64(&mut bytes, edge_count);
    put_u64(&mut bytes, node_count);
    put_u64(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 32]);
    bytes
}

fn delta_prefix(vertex_count: u64, edge_count: u64, change_count: u64) -> Vec<u8> {
    let mut bytes = header_with_magic(b"HOLOSDP\0", vertex_count);
    put_u64(&mut bytes, edge_count);
    put_u64(&mut bytes, change_count);
    put_u64(&mut bytes, 0);
    put_u64(&mut bytes, 0);
    bytes.extend_from_slice(&[0; 64]);
    bytes
}

fn header(vertex_count: u64) -> Vec<u8> {
    header_with_magic(SNAPSHOT_MAGIC, vertex_count)
}

fn header_with_magic(magic: &[u8; 8], vertex_count: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(magic);
    put_u16(&mut bytes, VERSION);
    bytes.push(F64_BITS_CODEC);
    put_u64(&mut bytes, 1);
    put_u32(&mut bytes, 2);
    bytes.push(0);
    put_u64(&mut bytes, vertex_count);
    bytes
}

fn node_header(bytes: &mut Vec<u8>, counts: [u64; 7]) {
    let [
        vertices,
        edges,
        separator,
        protected,
        children,
        edge_columns,
        bars,
    ] = counts;
    bytes.extend_from_slice(&[0; 32]);
    bytes.push(0);
    for value in [vertices, edges, separator, protected, children, 2] {
        put_u64(bytes, value);
    }
    put_u64(bytes, edge_columns);
    put_u64(bytes, 0);
    put_u64(bytes, bars);
    put_u64(bytes, 0);
}

fn small_limits() -> ProofLimits {
    ProofLimits {
        max_bytes: 1024,
        max_nodes: 2,
        max_snapshots: 1,
        max_vertices: 2,
        max_edges: 4,
        max_triangles: 4,
        max_higher_simplices: 4,
        max_dimension: 1,
        max_terms: 8,
        max_references: 8,
        max_bars: 4,
    }
}

#[test]
fn parameter_header_applies_vertex_limit_before_body_decode() {
    let bytes = header(3);
    let limits = small_limits();
    let mut reader = Reader::new(&bytes, limits, SNAPSHOT_MAGIC).unwrap();
    let error = reader.header(limits).unwrap_err();
    assert!(error.to_string().contains("index vertex count"));
}

#[test]
fn snapshot_edge_count_must_fit_remaining_bytes_before_reserve() {
    let bytes = snapshot_prefix(1, 2, 0);
    let error = decode_snapshot(&bytes, small_limits()).unwrap_err();
    assert!(error.to_string().contains("snapshot edges require"));
}

#[test]
fn delta_change_count_must_fit_remaining_bytes_before_reserve() {
    let bytes = delta_prefix(1, 1, 1);
    let error = decode_delta(&bytes, small_limits()).unwrap_err();
    assert!(error.to_string().contains("delta edge changes require"));
}

#[test]
fn raw_separator_and_child_counts_are_bounded_before_payload_decode() {
    for (separator, children, message) in [
        (u64::MAX, 0_u64, "interface separator count"),
        (0_u64, u64::MAX, "interface child count"),
    ] {
        let mut bytes = snapshot_prefix(1, 0, 1);
        node_header(&mut bytes, [1, 0, separator, 0, children, 0, 0]);
        let error = decode_snapshot(&bytes, small_limits()).unwrap_err();
        assert!(
            error.to_string().contains(message),
            "expected {message}, got {error}"
        );
    }
}

#[test]
fn term_count_must_fit_remaining_bytes_before_reserve() {
    let mut bytes = snapshot_prefix(1, 0, 1);
    node_header(&mut bytes, [1, 0, 0, 0, 0, 1, 0]);
    put_u64(&mut bytes, 0);
    put_u64(&mut bytes, 4);
    let error = decode_snapshot(&bytes, small_limits()).unwrap_err();
    assert!(
        error.to_string().contains("index proof terms require"),
        "unexpected error: {error}"
    );
}

#[test]
fn node_payload_collections_require_remaining_bytes_before_reserve() {
    let cases = [
        (1, 0, 0, "interface index list"),
        (0, 1, 0, "interface child references"),
        (0, 0, 1, "index proof bars"),
    ];
    for (vertices, children, bars, message) in cases {
        let mut bytes = snapshot_prefix(1, 0, 1);
        node_header(&mut bytes, [vertices, 0, 0, 0, children, 0, bars]);
        let error = decode_snapshot(&bytes, small_limits()).unwrap_err();
        assert!(
            error.to_string().contains(message),
            "expected {message}, got {error}"
        );
    }
}
