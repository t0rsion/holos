use crate::{
    Graph, ProofBar, ProofColumn, ProofEdge, ProofError, ProofLimits, ProofTerm,
    canonicalize_diagram, check_column,
};

use super::graded::check_graded_diagram;
use super::model::{Delta, InterfaceMode, InterfaceProof, Snapshot};
use super::wire_reader::Reader;

pub(super) const SNAPSHOT_MAGIC: &[u8; 8] = b"HOLOSIP\0";
const DELTA_MAGIC: &[u8; 8] = b"HOLOSDP\0";
pub(super) const VERSION: u16 = 4;
pub(super) const F64_BITS_CODEC: u8 = 1;

pub(super) fn decode_snapshot(bytes: &[u8], limits: ProofLimits) -> Result<Snapshot, ProofError> {
    let mut reader = Reader::new(bytes, limits, SNAPSHOT_MAGIC)?;
    let (max_dim, modulus, threshold, vertex_count) = reader.header(limits)?;
    let snapshot = decode_snapshot_body(
        &mut reader,
        max_dim,
        modulus,
        threshold,
        vertex_count,
        limits,
    )?;
    reader.finish()?;
    Ok(snapshot)
}

fn decode_snapshot_body(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Snapshot, ProofError> {
    let edge_count = reader.bounded_usize("snapshot edge count", limits.max_edges)?;
    let node_count = reader.bounded_usize("snapshot node count", limits.max_nodes)?;
    let bar_count = reader.bounded_usize("snapshot bar count", limits.max_bars)?;
    let root = reader.array32()?;
    let edges = decode_edges(reader, vertex_count, edge_count)?;
    let graph = Graph::new(vertex_count, &edges)?;
    let mut totals = Totals::default();
    let nodes = decode_nodes(reader, node_count, max_dim, modulus, limits, &mut totals)?;
    let diagram = decode_diagram(reader, bar_count, max_dim)?;
    Ok(Snapshot {
        max_dim,
        modulus,
        threshold,
        graph,
        root,
        nodes,
        diagram,
    })
}

pub(super) fn decode_delta(bytes: &[u8], limits: ProofLimits) -> Result<Delta, ProofError> {
    let mut reader = Reader::new(bytes, limits, DELTA_MAGIC)?;
    let (max_dim, modulus, threshold, vertex_count) = reader.header(limits)?;
    let delta = decode_delta_body(
        &mut reader,
        max_dim,
        modulus,
        threshold,
        vertex_count,
        limits,
    )?;
    reader.finish()?;
    Ok(delta)
}

fn decode_delta_body(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Delta, ProofError> {
    let edge_count = reader.bounded_usize("delta edge count", limits.max_edges)?;
    let change_count = reader.bounded_usize("delta edge-change count", limits.max_edges)?;
    let node_count = reader.bounded_usize("delta node count", limits.max_nodes)?;
    let bar_count = reader.bounded_usize("delta bar count", limits.max_bars)?;
    let old_root = reader.array32()?;
    let new_root = reader.array32()?;
    let edge_changes = decode_edge_changes(reader, edge_count, change_count)?;
    let mut totals = Totals::default();
    let nodes = decode_nodes(reader, node_count, max_dim, modulus, limits, &mut totals)?;
    let diagram = decode_diagram(reader, bar_count, max_dim)?;
    Ok(Delta {
        max_dim,
        modulus,
        threshold,
        vertex_count,
        edge_count,
        old_root,
        new_root,
        edge_changes,
        nodes,
        diagram,
    })
}

fn decode_edge_changes(
    reader: &mut Reader<'_>,
    edge_count: usize,
    change_count: usize,
) -> Result<Vec<(usize, f64)>, ProofError> {
    if edge_count == 0 && change_count != 0 {
        return Err(ProofError::new(
            "an empty index cannot contain delta edge changes",
        ));
    }
    let mut edge_changes = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        let position = reader.bounded_usize("delta edge position", edge_count.saturating_sub(1))?;
        let value = f64::from_bits(reader.u64()?);
        if !value.is_finite() || value < 0.0 {
            return Err(ProofError::new(
                "delta edge value is not finite and non-negative",
            ));
        }
        edge_changes.push((position, value));
    }
    if !edge_changes.windows(2).all(|pair| pair[0].0 < pair[1].0) {
        return Err(ProofError::new("delta edge changes are not canonical"));
    }
    Ok(edge_changes)
}

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    count: usize,
) -> Result<Vec<ProofEdge>, ProofError> {
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        let edge = ProofEdge {
            u: reader.usize()?,
            v: reader.usize()?,
            value: f64::from_bits(reader.u64()?),
        };
        if edge.u >= edge.v || edge.v >= vertex_count || !edge.value.is_finite() || edge.value < 0.0
        {
            return Err(ProofError::new("snapshot edge is not canonical"));
        }
        edges.push(edge);
    }
    if !edges
        .windows(2)
        .all(|pair| (pair[0].u, pair[0].v) < (pair[1].u, pair[1].v))
    {
        return Err(ProofError::new("snapshot edges are not in canonical order"));
    }
    Ok(edges)
}

#[derive(Default)]
struct Totals {
    vertices: usize,
    edge_positions: usize,
    simplex_columns: usize,
    terms: usize,
}

struct NodeWireHeader {
    digest: [u8; 32],
    mode: InterfaceMode,
    vertices: usize,
    edges: usize,
    separator: usize,
    protected: usize,
    children: usize,
    column_counts: Vec<usize>,
    bars: usize,
    relative_bytes: usize,
}

struct RawNodeCounts {
    vertices: usize,
    edges: usize,
    separator: usize,
    protected: usize,
    children: usize,
    boundaries: usize,
}

fn decode_nodes(
    reader: &mut Reader<'_>,
    count: usize,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
    totals: &mut Totals,
) -> Result<Vec<InterfaceProof>, ProofError> {
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        nodes.push(decode_node(reader, max_dim, modulus, limits, totals)?);
    }
    Ok(nodes)
}

fn decode_node(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
    totals: &mut Totals,
) -> Result<InterfaceProof, ProofError> {
    let header = decode_node_header(reader, max_dim, limits)?;
    record_node_totals(totals, &header, limits)?;
    decode_node_payload(reader, max_dim, modulus, limits, totals, header)
}

fn decode_node_payload(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
    totals: &mut Totals,
    header: NodeWireHeader,
) -> Result<InterfaceProof, ProofError> {
    let vertices = decode_usizes(reader, header.vertices)?;
    let edge_positions = decode_usizes(reader, header.edges)?;
    let separator = decode_usizes(reader, header.separator)?;
    let protected_vertices = decode_usizes(reader, header.protected)?;
    let children = decode_ids(reader, header.children)?;
    let graded_columns =
        decode_graded_columns(reader, &header.column_counts, modulus, limits, totals)?;
    let diagram = decode_diagram(reader, header.bars, max_dim)?;
    let relative_artifact = reader.take(header.relative_bytes)?.to_vec();
    Ok(InterfaceProof {
        digest: header.digest,
        vertices,
        edge_positions,
        separator,
        protected_vertices,
        children,
        mode: header.mode,
        graded_columns,
        relative_artifact,
        diagram,
    })
}

fn decode_node_header(
    reader: &mut Reader<'_>,
    max_dim: usize,
    limits: ProofLimits,
) -> Result<NodeWireHeader, ProofError> {
    let digest = reader.array32()?;
    let mode = decode_interface_mode(reader.u8()?)?;
    let raw = decode_raw_node_counts(reader, limits)?;
    if raw.boundaries != max_dim + 1 {
        return Err(ProofError::new(
            "interface boundary count differs from the proof dimension",
        ));
    }
    Ok(NodeWireHeader {
        digest,
        mode,
        vertices: raw.vertices,
        edges: raw.edges,
        separator: raw.separator,
        protected: raw.protected,
        children: raw.children,
        column_counts: decode_column_counts(reader, raw.boundaries, limits)?,
        bars: reader.bounded_usize("interface bar count", limits.max_bars)?,
        relative_bytes: reader.bounded_usize("relative interface byte count", limits.max_bytes)?,
    })
}

fn decode_interface_mode(tag: u8) -> Result<InterfaceMode, ProofError> {
    match tag {
        0 => Ok(InterfaceMode::Materialized),
        1 => Ok(InterfaceMode::Disjoint),
        2 => Ok(InterfaceMode::ZeroSimplex),
        3 => Ok(InterfaceMode::ZeroCone),
        4 => Ok(InterfaceMode::Relative),
        _ => Err(ProofError::new("invalid interface-mode tag")),
    }
}

fn decode_raw_node_counts(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<RawNodeCounts, ProofError> {
    Ok(RawNodeCounts {
        vertices: reader.usize()?,
        edges: reader.usize()?,
        separator: reader.usize()?,
        protected: reader.bounded_usize("protected vertex count", limits.max_vertices)?,
        children: reader.usize()?,
        boundaries: reader.usize()?,
    })
}

fn decode_column_counts(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProofLimits,
) -> Result<Vec<usize>, ProofError> {
    (1..=count)
        .map(|dimension| {
            reader.bounded_usize(
                "interface simplex columns",
                simplex_limit(dimension, limits),
            )
        })
        .collect()
}

fn simplex_limit(dimension: usize, limits: ProofLimits) -> usize {
    match dimension {
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn record_node_totals(
    totals: &mut Totals,
    header: &NodeWireHeader,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    totals.vertices = bounded_sum(
        totals.vertices,
        header.vertices,
        limits.max_vertices.saturating_mul(limits.max_nodes),
        "interface vertices",
    )?;
    totals.edge_positions = bounded_sum(
        totals.edge_positions,
        header.edges,
        limits.max_references,
        "interface edge positions",
    )?;
    for &count in &header.column_counts {
        totals.simplex_columns = bounded_sum(
            totals.simplex_columns,
            count,
            limits.max_references,
            "interface simplex columns",
        )?;
    }
    Ok(())
}

fn decode_ids(reader: &mut Reader<'_>, count: usize) -> Result<Vec<[u8; 32]>, ProofError> {
    (0..count).map(|_| reader.array32()).collect()
}

fn decode_graded_columns(
    reader: &mut Reader<'_>,
    counts: &[usize],
    modulus: u32,
    limits: ProofLimits,
    totals: &mut Totals,
) -> Result<Vec<Vec<ProofColumn>>, ProofError> {
    counts
        .iter()
        .map(|&count| decode_columns(reader, count, modulus, limits.max_terms, &mut totals.terms))
        .collect()
}

fn decode_usizes(reader: &mut Reader<'_>, count: usize) -> Result<Vec<usize>, ProofError> {
    let mut output = Vec::with_capacity(count);
    for _ in 0..count {
        output.push(reader.usize()?);
    }
    Ok(output)
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<Vec<ProofColumn>, ProofError> {
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        let term_count = reader.usize()?;
        *total_terms = bounded_sum(*total_terms, term_count, term_limit, "index proof terms")?;
        let mut terms = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            terms.push(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            });
        }
        check_column(target, &terms, modulus)?;
        columns.push(ProofColumn { terms });
    }
    Ok(columns)
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    count: usize,
    max_dim: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let mut diagram = Vec::with_capacity(count);
    for _ in 0..count {
        diagram.push(ProofBar {
            dimension: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    check_graded_diagram(&diagram, max_dim)?;
    canonicalize_diagram(&mut diagram);
    Ok(diagram)
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let total = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new(format!("{label} overflow")))?;
    if total > limit {
        return Err(ProofError::new(format!(
            "{label} count {total} exceeds the limit {limit}"
        )));
    }
    Ok(total)
}
