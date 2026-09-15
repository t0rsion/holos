use sha2::{Digest, Sha256};

use crate::{F64_BITS_CODEC, MAGIC, VERSION};

use super::graph::check_column;
use super::model::{
    AtomProof, ProofBar, ProofColumn, ProofEdge, ProofError, ProofLimits, ProofTerm, SnapshotProof,
};

const NODE_HEADER_BYTES: usize = 32 + 4 * 8;
const SNAPSHOT_HEADER_BYTES: usize = 4 * 8;
const WIRE_USIZE_BYTES: usize = 8;
const EDGE_KEY_BYTES: usize = 2 * WIRE_USIZE_BYTES;
const PROOF_EDGE_BYTES: usize = 2 * WIRE_USIZE_BYTES + 8;
const PROOF_BAR_BYTES: usize = 3 * 8;
const COLUMN_COUNT_BYTES: usize = WIRE_USIZE_BYTES;
const PROOF_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;

pub(crate) fn decode_bundle_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC {
        return Err(ProofError::new("wrong magic bytes"));
    }
    if reader.u16()? != VERSION {
        return Err(ProofError::new("unsupported wire version"));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported scalar codec"));
    }
    Ok(())
}

pub(crate) fn decode_nodes(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProofLimits,
    modulus: u32,
    totals: &mut DecodeTotals,
) -> Result<Vec<AtomProof>, ProofError> {
    reader.require_bytes(count, NODE_HEADER_BYTES, "reduction node headers")?;
    (0..count)
        .map(|_| decode_node(reader, limits, modulus, totals))
        .collect()
}

pub(crate) fn decode_snapshots(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Vec<SnapshotProof>, ProofError> {
    reader.require_bytes(count, SNAPSHOT_HEADER_BYTES, "snapshot headers")?;
    (0..count)
        .map(|_| decode_snapshot(reader, limits, totals))
        .collect()
}

pub(crate) fn encode_node(output: &mut Vec<u8>, node: &AtomProof) -> Result<(), ProofError> {
    output.extend_from_slice(&node.digest);
    put_usize(output, node.vertices.len())?;
    put_usize(output, node.edges.len())?;
    put_usize(output, node.edge_columns.len())?;
    put_usize(output, node.triangle_columns.len())?;
    encode_usizes(output, &node.vertices)?;
    encode_edge_keys(output, &node.edges)?;
    encode_columns(output, &node.edge_columns)?;
    encode_columns(output, &node.triangle_columns)?;
    Ok(())
}

fn encode_columns(output: &mut Vec<u8>, columns: &[ProofColumn]) -> Result<(), ProofError> {
    for column in columns {
        put_usize(output, column.terms.len())?;
        for term in &column.terms {
            put_usize(output, term.index)?;
            put_u32(output, term.coefficient);
        }
    }
    Ok(())
}

pub(crate) fn encode_snapshot(
    output: &mut Vec<u8>,
    snapshot: &SnapshotProof,
) -> Result<(), ProofError> {
    put_usize(output, snapshot.vertex_count)?;
    put_usize(output, snapshot.edges.len())?;
    put_usize(output, snapshot.atom_refs.len())?;
    put_usize(output, snapshot.diagram.len())?;
    encode_proof_edges(output, &snapshot.edges)?;
    for digest in &snapshot.atom_refs {
        output.extend_from_slice(digest);
    }
    encode_bars(output, &snapshot.diagram)?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), ProofError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_edge_keys(output: &mut Vec<u8>, edges: &[[usize; 2]]) -> Result<(), ProofError> {
    for &[u, v] in edges {
        put_usize(output, u)?;
        put_usize(output, v)?;
    }
    Ok(())
}

fn encode_proof_edges(output: &mut Vec<u8>, edges: &[ProofEdge]) -> Result<(), ProofError> {
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
        put_u64(output, edge.value.to_bits());
    }
    Ok(())
}

fn encode_bars(output: &mut Vec<u8>, bars: &[ProofBar]) -> Result<(), ProofError> {
    for bar in bars {
        put_usize(output, bar.dimension)?;
        put_u64(output, bar.birth.to_bits());
        put_u64(output, bar.death.to_bits());
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct DecodeTotals {
    vertices: usize,
    edges: usize,
    edge_columns: usize,
    triangles: usize,
    terms: usize,
    references: usize,
    bars: usize,
}

struct NodeHeader {
    digest: [u8; 32],
    vertices: usize,
    edges: usize,
    edge_columns: usize,
    triangle_columns: usize,
}

struct SnapshotHeader {
    vertices: usize,
    edges: usize,
    references: usize,
    bars: usize,
}

fn decode_node(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    modulus: u32,
    totals: &mut DecodeTotals,
) -> Result<AtomProof, ProofError> {
    let header = decode_node_header(reader, limits)?;
    record_node_totals(totals, &header, limits)?;
    let vertices = decode_usizes(reader, header.vertices, "node vertices")?;
    let edges = decode_edge_keys(reader, header.edges)?;
    let edge_columns = decode_columns(
        reader,
        header.edge_columns,
        limits.max_edges,
        modulus,
        limits.max_terms,
        &mut totals.terms,
        "edge columns",
    )?;
    let triangle_columns = decode_columns(
        reader,
        header.triangle_columns,
        limits.max_triangles,
        modulus,
        limits.max_terms,
        &mut totals.terms,
        "triangle columns",
    )?;
    finish_node(
        header.digest,
        vertices,
        edges,
        edge_columns,
        triangle_columns,
        modulus,
    )
}

fn decode_node_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<NodeHeader, ProofError> {
    Ok(NodeHeader {
        digest: reader.array32()?,
        vertices: reader.bounded_usize("node vertex count", limits.max_vertices)?,
        edges: reader.bounded_usize("node edge count", limits.max_edges)?,
        edge_columns: reader.bounded_usize("node edge-column count", limits.max_edges)?,
        triangle_columns: reader
            .bounded_usize("node triangle-column count", limits.max_triangles)?,
    })
}

fn record_node_totals(
    totals: &mut DecodeTotals,
    header: &NodeHeader,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    totals.vertices = bounded_sum(
        totals.vertices,
        header.vertices,
        limits.max_vertices.saturating_mul(limits.max_nodes),
        "node vertices",
    )?;
    totals.edges = bounded_sum(totals.edges, header.edges, limits.max_edges, "node edges")?;
    totals.edge_columns = bounded_sum(
        totals.edge_columns,
        header.edge_columns,
        limits.max_edges,
        "edge columns",
    )?;
    totals.triangles = bounded_sum(
        totals.triangles,
        header.triangle_columns,
        limits.max_triangles,
        "triangle columns",
    )?;
    Ok(())
}

fn decode_usizes(
    reader: &mut Reader<'_>,
    count: usize,
    label: &str,
) -> Result<Vec<usize>, ProofError> {
    reader.require_bytes(count, WIRE_USIZE_BYTES, label)?;
    (0..count).map(|_| reader.usize()).collect()
}

fn decode_edge_keys(reader: &mut Reader<'_>, count: usize) -> Result<Vec<[usize; 2]>, ProofError> {
    reader.require_bytes(count, EDGE_KEY_BYTES, "node edges")?;
    (0..count)
        .map(|_| Ok([reader.usize()?, reader.usize()?]))
        .collect()
}

fn finish_node(
    digest: [u8; 32],
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    edge_columns: Vec<ProofColumn>,
    triangle_columns: Vec<ProofColumn>,
    modulus: u32,
) -> Result<AtomProof, ProofError> {
    let node = AtomProof {
        digest,
        vertices,
        edges,
        edge_columns,
        triangle_columns,
    };
    node.check_shape(modulus)?;
    if node.compute_digest() != digest {
        return Err(ProofError::new("reduction-node digest does not match"));
    }
    Ok(node)
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    column_limit: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
    label: &str,
) -> Result<Vec<ProofColumn>, ProofError> {
    check_column_count(reader, count, column_limit, label)?;
    (0..count)
        .map(|target| decode_column(reader, target, modulus, term_limit, total_terms))
        .collect()
}

fn check_column_count(
    reader: &Reader<'_>,
    count: usize,
    limit: usize,
    label: &str,
) -> Result<(), ProofError> {
    if count > limit {
        return Err(ProofError::new(format!(
            "{label} {count} exceed the limit {limit}"
        )));
    }
    reader.require_bytes(count, COLUMN_COUNT_BYTES, label)
}

fn decode_column(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<ProofColumn, ProofError> {
    let term_count = reader.bounded_usize("change term count", term_limit)?;
    *total_terms = bounded_sum(*total_terms, term_count, term_limit, "change terms")?;
    let terms = decode_terms(reader, term_count)?;
    check_column(target, &terms, modulus)?;
    Ok(ProofColumn { terms })
}

fn decode_terms(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ProofTerm>, ProofError> {
    reader.require_bytes(count, PROOF_TERM_BYTES, "change terms")?;
    (0..count)
        .map(|_| {
            Ok(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_snapshot(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    totals: &mut DecodeTotals,
) -> Result<SnapshotProof, ProofError> {
    let header = decode_snapshot_header(reader, limits)?;
    record_snapshot_totals(totals, &header, limits)?;
    let edges = decode_proof_edges(reader, header.edges)?;
    reader.require_bytes(header.references, 32, "snapshot references")?;
    let atom_refs = (0..header.references)
        .map(|_| reader.array32())
        .collect::<Result<Vec<_>, _>>()?;
    let diagram = decode_bars(reader, header.bars)?;
    SnapshotProof::new(header.vertices, edges, atom_refs, diagram)
}

fn decode_snapshot_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<SnapshotHeader, ProofError> {
    Ok(SnapshotHeader {
        vertices: reader.bounded_usize("snapshot vertex count", limits.max_vertices)?,
        edges: reader.bounded_usize("snapshot edge count", limits.max_edges)?,
        references: reader.bounded_usize("snapshot reference count", limits.max_references)?,
        bars: reader.bounded_usize("snapshot bar count", limits.max_bars)?,
    })
}

fn record_snapshot_totals(
    totals: &mut DecodeTotals,
    header: &SnapshotHeader,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    totals.edges = bounded_sum(
        totals.edges,
        header.edges,
        limits.max_edges,
        "snapshot edges",
    )?;
    totals.references = bounded_sum(
        totals.references,
        header.references,
        limits.max_references,
        "node references",
    )?;
    totals.bars = bounded_sum(totals.bars, header.bars, limits.max_bars, "diagram bars")?;
    Ok(())
}

fn decode_proof_edges(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ProofEdge>, ProofError> {
    reader.require_bytes(count, PROOF_EDGE_BYTES, "snapshot edges")?;
    (0..count)
        .map(|_| {
            Ok(ProofEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                value: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}

fn decode_bars(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ProofBar>, ProofError> {
    reader.require_bytes(count, PROOF_BAR_BYTES, "diagram bars")?;
    (0..count)
        .map(|_| {
            Ok(ProofBar {
                dimension: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}

pub(crate) fn digest_columns(hash: &mut Sha256, columns: &[ProofColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProofError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

pub(crate) fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), ProofError> {
    let value = u64::try_from(value)
        .map_err(|_| ProofError::new("integer does not fit the wire format"))?;
    put_u64(output, value);
    Ok(())
}

pub(crate) fn put_optional_f64(output: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => output.push(0),
        Some(value) => {
            output.push(1);
            put_u64(output, value.to_bits());
        }
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(crate) fn require_bytes(
        &self,
        count: usize,
        minimum_width: usize,
        label: &str,
    ) -> Result<(), ProofError> {
        let required = count.checked_mul(minimum_width).ok_or_else(|| {
            ProofError::new(format!("{label} minimum byte count overflows usize"))
        })?;
        if required > self.remaining() {
            return Err(ProofError::new(format!(
                "{label} requires at least {required} bytes, only {} remain",
                self.remaining()
            )));
        }
        Ok(())
    }

    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(crate) fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?).map_err(|_| ProofError::new("wire integer does not fit usize"))
    }

    pub(crate) fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProofError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(crate) fn optional_f64(&mut self) -> Result<Option<f64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(ProofError::new(format!("unknown optional-float tag {tag}"))),
        }
    }

    pub(crate) fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
