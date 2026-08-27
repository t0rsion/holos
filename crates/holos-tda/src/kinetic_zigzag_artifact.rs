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

impl KineticZigzagArtifact {
    /// Build one artifact and return the complete checked zigzag.
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

    /// Recompute the exact event cells, maps, ranks, and interval decomposition.
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
        if bytes.len() > limits.max_bytes || bytes.len() < 32 {
            return Err(Error::InvalidInput(
                "kinetic zigzag artifact exceeds its byte limit or is truncated".into(),
            ));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
            return Err(Error::InvalidInput(
                "unsupported kinetic zigzag artifact".into(),
            ));
        }
        let vertex_count = reader.bounded_usize("vertex count", limits.cohomology.max_vertices)?;
        let edge_count = reader.bounded_usize("edge count", limits.kinetic.max_edges)?;
        if edge_count > reader.remaining() / 32 {
            return Err(Error::InvalidInput(
                "kinetic zigzag edge count exceeds the remaining bytes".into(),
            ));
        }
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            edges.push(KineticEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                intercept: f64::from_bits(reader.u64()?),
                velocity: f64::from_bits(reader.u64()?),
            });
        }
        let start = f64::from_bits(reader.u64()?);
        let end = f64::from_bits(reader.u64()?);
        let dimension = reader.bounded_usize("dimension", limits.cohomology.max_dimension)?;
        let scale = f64::from_bits(reader.u64()?);
        let modulus = reader.u32()?;
        let persistent_ties = reader.usize()?;
        let node_ranks = reader.usizes("node ranks", limits.zigzag.max_nodes)?;
        let node_active_edges = reader.usizes("node edge counts", limits.zigzag.max_nodes)?;
        let arrow_ranks =
            reader.usizes("arrow ranks", limits.zigzag.max_nodes.saturating_sub(1))?;
        let maximum_ranks = node_ranks
            .len()
            .checked_mul(node_ranks.len())
            .ok_or_else(|| Error::InvalidInput("kinetic zigzag rank count overflows".into()))?;
        let generalized_ranks = reader.usizes("generalized ranks", maximum_ranks)?;
        let maximum_intervals = node_ranks
            .len()
            .checked_mul(node_ranks.len().saturating_add(1))
            .map(|value| value / 2)
            .ok_or_else(|| Error::InvalidInput("kinetic zigzag interval count overflows".into()))?;
        let interval_count = reader.bounded_usize("interval count", maximum_intervals)?;
        let mut intervals = Vec::with_capacity(interval_count);
        for _ in 0..interval_count {
            intervals.push(KineticZigzagIntervalClaim {
                start: reader.usize()?,
                end: reader.usize()?,
                multiplicity: reader.usize()?,
            });
        }
        let digest = reader.array32()?;
        if reader.remaining() != 0 {
            return Err(Error::InvalidInput(
                "trailing bytes follow the kinetic zigzag artifact".into(),
            ));
        }
        let artifact = Self {
            vertex_count,
            edges,
            start,
            end,
            dimension,
            scale,
            modulus,
            persistent_ties,
            node_ranks,
            node_active_edges,
            arrow_ranks,
            generalized_ranks,
            intervals,
            digest,
        };
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
        output.extend_from_slice(MAGIC);
        write_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        write_usize(&mut output, self.vertex_count)?;
        write_usize(&mut output, self.edges.len())?;
        for edge in &self.edges {
            write_usize(&mut output, edge.u)?;
            write_usize(&mut output, edge.v)?;
            write_u64(&mut output, edge.intercept.to_bits());
            write_u64(&mut output, edge.velocity.to_bits());
        }
        write_u64(&mut output, self.start.to_bits());
        write_u64(&mut output, self.end.to_bits());
        write_usize(&mut output, self.dimension)?;
        write_u64(&mut output, self.scale.to_bits());
        write_u32(&mut output, self.modulus);
        write_usize(&mut output, self.persistent_ties)?;
        write_usizes(&mut output, &self.node_ranks)?;
        write_usizes(&mut output, &self.node_active_edges)?;
        write_usizes(&mut output, &self.arrow_ranks)?;
        write_usizes(&mut output, &self.generalized_ranks)?;
        write_usize(&mut output, self.intervals.len())?;
        for interval in &self.intervals {
            write_usize(&mut output, interval.start)?;
            write_usize(&mut output, interval.end)?;
            write_usize(&mut output, interval.multiplicity)?;
        }
        Ok(output)
    }
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
