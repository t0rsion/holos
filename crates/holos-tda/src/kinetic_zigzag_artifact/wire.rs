use sha2::{Digest, Sha256};

use crate::{Error, KineticEdge, Result};

use super::{KineticZigzagArtifact, KineticZigzagArtifactLimits, KineticZigzagIntervalClaim};

const MAGIC: &[u8; 8] = b"HOLOSZZ\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

struct KineticZigzagHeader {
    vertex_count: usize,
    edges: Vec<KineticEdge>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    persistent_ties: usize,
}

struct KineticZigzagRanks {
    node_ranks: Vec<usize>,
    node_active_edges: Vec<usize>,
    arrow_ranks: Vec<usize>,
    generalized_ranks: Vec<usize>,
}

impl KineticZigzagArtifact {
    /// Encode canonical `HOLOSZZ` version 1 bytes.
    pub fn encode(&self, limits: KineticZigzagArtifactLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "kinetic zigzag artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSZZ` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: KineticZigzagArtifactLimits) -> Result<Self> {
        validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_prefix(&mut reader)?;
        let header = decode_header(&mut reader, limits)?;
        let ranks = decode_ranks(&mut reader, limits)?;
        let intervals = decode_intervals(&mut reader, ranks.node_ranks.len())?;
        let digest = decode_trailer(&mut reader)?;
        let artifact = Self {
            vertex_count: header.vertex_count,
            edges: header.edges,
            start: header.start,
            end: header.end,
            dimension: header.dimension,
            scale: header.scale,
            modulus: header.modulus,
            persistent_ties: header.persistent_ties,
            node_ranks: ranks.node_ranks,
            node_active_edges: ranks.node_active_edges,
            arrow_ranks: ranks.arrow_ranks,
            generalized_ranks: ranks.generalized_ranks,
            intervals,
            digest,
        };
        validate_decoded_artifact(&artifact, bytes, limits)?;
        Ok(artifact)
    }

    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = self.encode_payload()?;
        let mut hash = Sha256::new();
        hash.update(b"holos-kinetic-zigzag-v1");
        hash.update(payload);
        Ok(hash.finalize().into())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        encode_prefix(&mut output);
        encode_header(&mut output, self)?;
        encode_edges(&mut output, &self.edges)?;
        encode_ranks(&mut output, self)?;
        encode_intervals(&mut output, &self.intervals)?;
        Ok(output)
    }
}

fn validate_artifact_size(bytes: &[u8], limits: KineticZigzagArtifactLimits) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "kinetic zigzag artifact exceeds its byte limit or is truncated".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<()> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(Error::InvalidInput(
            "unsupported kinetic zigzag artifact".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: KineticZigzagArtifactLimits,
) -> Result<KineticZigzagHeader> {
    Ok(KineticZigzagHeader {
        vertex_count: reader.bounded_usize("vertex count", limits.cohomology.max_vertices)?,
        edges: decode_edges(reader, limits)?,
        start: f64::from_bits(reader.u64()?),
        end: f64::from_bits(reader.u64()?),
        dimension: reader.bounded_usize("dimension", limits.cohomology.max_dimension)?,
        scale: f64::from_bits(reader.u64()?),
        modulus: reader.u32()?,
        persistent_ties: reader.usize()?,
    })
}

fn decode_edges(
    reader: &mut Reader<'_>,
    limits: KineticZigzagArtifactLimits,
) -> Result<Vec<KineticEdge>> {
    let count = reader.bounded_usize("edge count", limits.kinetic.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput(
            "kinetic zigzag edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| decode_edge(reader)).collect()
}

fn decode_edge(reader: &mut Reader<'_>) -> Result<KineticEdge> {
    Ok(KineticEdge {
        u: reader.usize()?,
        v: reader.usize()?,
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}

fn decode_ranks(
    reader: &mut Reader<'_>,
    limits: KineticZigzagArtifactLimits,
) -> Result<KineticZigzagRanks> {
    let node_ranks = reader.usizes("node ranks", limits.zigzag.max_nodes)?;
    let node_active_edges = reader.usizes("node edge counts", limits.zigzag.max_nodes)?;
    let arrow_ranks = reader.usizes("arrow ranks", limits.zigzag.max_nodes.saturating_sub(1))?;
    let generalized_ranks = reader.usizes("generalized ranks", rank_square(node_ranks.len())?)?;
    Ok(KineticZigzagRanks {
        node_ranks,
        node_active_edges,
        arrow_ranks,
        generalized_ranks,
    })
}

fn rank_square(nodes: usize) -> Result<usize> {
    nodes
        .checked_mul(nodes)
        .ok_or_else(|| Error::InvalidInput("kinetic zigzag rank count overflows".into()))
}

fn decode_intervals(
    reader: &mut Reader<'_>,
    nodes: usize,
) -> Result<Vec<KineticZigzagIntervalClaim>> {
    let count = reader.bounded_usize("interval count", maximum_intervals(nodes)?)?;
    (0..count).map(|_| decode_interval(reader)).collect()
}

fn maximum_intervals(nodes: usize) -> Result<usize> {
    nodes
        .checked_mul(nodes.saturating_add(1))
        .map(|value| value / 2)
        .ok_or_else(|| Error::InvalidInput("kinetic zigzag interval count overflows".into()))
}

fn decode_interval(reader: &mut Reader<'_>) -> Result<KineticZigzagIntervalClaim> {
    Ok(KineticZigzagIntervalClaim {
        start: reader.usize()?,
        end: reader.usize()?,
        multiplicity: reader.usize()?,
    })
}

fn decode_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the kinetic zigzag artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

fn validate_decoded_artifact(
    artifact: &KineticZigzagArtifact,
    bytes: &[u8],
    limits: KineticZigzagArtifactLimits,
) -> Result<()> {
    if artifact.compute_digest()? != artifact.digest {
        return Err(Error::InvalidInput(
            "kinetic zigzag digest differs from its content".into(),
        ));
    }
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "kinetic zigzag encoding is not canonical".into(),
        ));
    }
    Ok(())
}

fn encode_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    write_u16(output, VERSION);
    output.push(F64_BITS_CODEC);
}

fn encode_header(output: &mut Vec<u8>, artifact: &KineticZigzagArtifact) -> Result<()> {
    write_usize(output, artifact.vertex_count)?;
    write_usize(output, artifact.edges.len())
}

fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdge]) -> Result<()> {
    for edge in edges {
        write_usize(output, edge.u)?;
        write_usize(output, edge.v)?;
        write_u64(output, edge.intercept.to_bits());
        write_u64(output, edge.velocity.to_bits());
    }
    Ok(())
}

fn encode_ranks(output: &mut Vec<u8>, artifact: &KineticZigzagArtifact) -> Result<()> {
    write_u64(output, artifact.start.to_bits());
    write_u64(output, artifact.end.to_bits());
    write_usize(output, artifact.dimension)?;
    write_u64(output, artifact.scale.to_bits());
    write_u32(output, artifact.modulus);
    write_usize(output, artifact.persistent_ties)?;
    write_usizes(output, &artifact.node_ranks)?;
    write_usizes(output, &artifact.node_active_edges)?;
    write_usizes(output, &artifact.arrow_ranks)?;
    write_usizes(output, &artifact.generalized_ranks)
}

fn encode_intervals(output: &mut Vec<u8>, intervals: &[KineticZigzagIntervalClaim]) -> Result<()> {
    write_usize(output, intervals.len())?;
    for interval in intervals {
        write_usize(output, interval.start)?;
        write_usize(output, interval.end)?;
        write_usize(output, interval.multiplicity)?;
    }
    Ok(())
}

fn write_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    write_usize(output, values.len())?;
    for value in values {
        write_usize(output, *value)?;
    }
    Ok(())
}

fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    write_u64(
        output,
        u64::try_from(value)
            .map_err(|_| Error::InvalidInput("kinetic zigzag integer exceeds u64".into()))?,
    );
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| Error::InvalidInput("truncated kinetic zigzag artifact".into()))?;
        let output = &self.bytes[self.position..end];
        self.position = end;
        Ok(output)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| Error::InvalidInput("kinetic zigzag integer exceeds usize".into()))
    }

    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(Error::InvalidInput(format!(
                "kinetic zigzag {name} exceeds the limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn usizes(&mut self, name: &str, maximum: usize) -> Result<Vec<usize>> {
        let count = self.bounded_usize(name, maximum)?;
        if count > self.remaining() / 8 {
            return Err(Error::InvalidInput(format!(
                "kinetic zigzag {name} exceed the remaining bytes"
            )));
        }
        (0..count).map(|_| self.usize()).collect()
    }

    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
