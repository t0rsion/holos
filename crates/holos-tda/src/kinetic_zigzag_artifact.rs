//! Self-contained certificates for exact kinetic cohomology zigzags.

use sha2::{Digest, Sha256};

use crate::{
    CohomologyLimits, Error, KineticEdge, KineticFiltration, KineticLimits, KineticZigzag, Result,
    ZigzagLimits,
};

const MAGIC: &[u8; 8] = b"HOLOSZZ\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Resource limits for kinetic zigzag artifacts and replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KineticZigzagArtifactLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Limits for the exact affine event schedule.
    pub kinetic: KineticLimits,
    /// Limits for each canonical cohomology computation.
    pub cohomology: CohomologyLimits,
    /// Limits for finite zigzag decomposition.
    pub zigzag: ZigzagLimits,
}

impl Default for KineticZigzagArtifactLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            kinetic: KineticLimits::default(),
            cohomology: CohomologyLimits::default(),
            zigzag: ZigzagLimits::default(),
        }
    }
}

/// One claimed interval-isotypic space in a kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KineticZigzagIntervalClaim {
    /// First zigzag node covered by the interval.
    pub start: usize,
    /// Last zigzag node covered by the interval, inclusive.
    pub end: usize,
    /// Number of indistinguishable copies.
    pub multiplicity: usize,
}

/// Size summary of one checked kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KineticZigzagArtifactSummary {
    /// Affine edge trajectory count.
    pub edges: usize,
    /// Alternating open-cell and event node count.
    pub nodes: usize,
    /// Exact restriction arrow count.
    pub arrows: usize,
    /// Nonzero interval-isotypic space count.
    pub intervals: usize,
    /// Sum of all interval multiplicities.
    pub interval_copies: usize,
}

/// Self-contained exact kinetic zigzag certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticZigzagArtifact {
    vertex_count: usize,
    edges: Vec<KineticEdge>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    persistent_ties: usize,
    node_ranks: Vec<usize>,
    node_active_edges: Vec<usize>,
    arrow_ranks: Vec<usize>,
    generalized_ranks: Vec<usize>,
    intervals: Vec<KineticZigzagIntervalClaim>,
    digest: [u8; 32],
}

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
    /// Build one artifact and return the checked zigzag.
    pub fn build(
        trajectory: &KineticFiltration,
        dimension: usize,
        scale: f64,
        modulus: u32,
        limits: KineticZigzagArtifactLimits,
    ) -> Result<(Self, KineticZigzag)> {
        let zigzag = trajectory.cohomology_zigzag(
            dimension,
            scale,
            modulus,
            limits.cohomology,
            limits.zigzag,
        )?;
        let mut artifact = Self::from_zigzag(trajectory, &zigzag);
        artifact.digest = artifact.compute_digest()?;
        Ok((artifact, zigzag))
    }

    fn from_zigzag(trajectory: &KineticFiltration, zigzag: &KineticZigzag) -> Self {
        Self {
            vertex_count: trajectory.vertex_count(),
            edges: trajectory.edges().to_vec(),
            start: trajectory.start(),
            end: trajectory.end(),
            dimension: zigzag.dimension,
            scale: zigzag.scale,
            modulus: zigzag.modulus,
            persistent_ties: zigzag.persistent_ties,
            node_ranks: zigzag.nodes.iter().map(|node| node.rank).collect(),
            node_active_edges: zigzag.nodes.iter().map(|node| node.active_edges).collect(),
            arrow_ranks: zigzag
                .arrows
                .iter()
                .map(|arrow| arrow.restriction.rank)
                .collect(),
            generalized_ranks: zigzag.barcode.generalized_ranks.clone(),
            intervals: zigzag
                .barcode
                .intervals
                .iter()
                .map(|interval| KineticZigzagIntervalClaim {
                    start: interval.start,
                    end: interval.end,
                    multiplicity: interval.multiplicity,
                })
                .collect(),
            digest: [0; 32],
        }
    }

    /// Recompute the zigzag and compare every stored claim.
    pub fn verify(&self, limits: KineticZigzagArtifactLimits) -> Result<()> {
        let trajectory = KineticFiltration::new(
            self.vertex_count,
            self.edges.clone(),
            self.start,
            self.end,
            limits.kinetic,
        )?;
        let (rebuilt, _) = Self::build(
            &trajectory,
            self.dimension,
            self.scale,
            self.modulus,
            limits,
        )?;
        if rebuilt != *self {
            return Err(Error::InvalidInput(
                "kinetic zigzag differs from exact replay".into(),
            ));
        }
        Ok(())
    }

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

    /// Number of graph vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Target cohomology dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Fixed filtration scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Claimed interval-isotypic spaces.
    pub fn intervals(&self) -> &[KineticZigzagIntervalClaim] {
        &self.intervals
    }

    /// Structural size of this artifact claim.
    pub fn summary(&self) -> KineticZigzagArtifactSummary {
        KineticZigzagArtifactSummary {
            edges: self.edges.len(),
            nodes: self.node_ranks.len(),
            arrows: self.arrow_ranks.len(),
            intervals: self.intervals.len(),
            interval_copies: self.intervals.iter().map(|item| item.multiplicity).sum(),
        }
    }

    fn compute_digest(&self) -> Result<[u8; 32]> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn trajectory() -> KineticFiltration {
        KineticFiltration::new(
            4,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.5,
                    velocity: 1.0,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 0.5,
                    velocity: 0.0,
                },
                KineticEdge {
                    u: 2,
                    v: 3,
                    intercept: 0.5,
                    velocity: 0.0,
                },
                KineticEdge {
                    u: 0,
                    v: 3,
                    intercept: 0.5,
                    velocity: 0.0,
                },
            ],
            0.0,
            1.0,
            KineticLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn artifact_round_trips_and_replays_the_complete_zigzag() {
        let limits = KineticZigzagArtifactLimits::default();
        let (artifact, zigzag) =
            KineticZigzagArtifact::build(&trajectory(), 1, 1.0, 3, limits).unwrap();
        let bytes = artifact.encode(limits).unwrap();
        let decoded = KineticZigzagArtifact::decode(&bytes, limits).unwrap();
        assert_eq!(decoded, artifact);
        assert_eq!(decoded.summary().nodes, zigzag.nodes.len());
        assert!(!decoded.intervals().is_empty());
    }

    #[test]
    fn mutations_and_truncations_are_rejected() {
        let limits = KineticZigzagArtifactLimits::default();
        let (artifact, _) = KineticZigzagArtifact::build(&trajectory(), 1, 1.0, 5, limits).unwrap();
        let bytes = artifact.encode(limits).unwrap();
        for position in [0, 8, bytes.len() / 2, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[position] ^= 0x40;
            assert!(KineticZigzagArtifact::decode(&changed, limits).is_err());
        }
        for length in 0..bytes.len().min(128) {
            assert!(KineticZigzagArtifact::decode(&bytes[..length], limits).is_err());
        }
    }
}
