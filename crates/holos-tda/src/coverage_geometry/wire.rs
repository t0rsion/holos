use sha2::{Digest, Sha256};

use super::{CoverageGeometry, CoverageGeometryLimits, PlanarPoint, geometry_error};
use crate::{CoverageSynthesisArtifact, CoverageSynthesisLimits, Result};

const MAGIC: &[u8; 8] = b"HOLOSGEO";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Decoder limits for a geometry-bound coverage artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GeometryBoundCoverageDecodeLimits {
    /// Largest accepted outer envelope.
    pub max_bytes: usize,
}

impl Default for GeometryBoundCoverageDecodeLimits {
    fn default() -> Self {
        Self { max_bytes: 1 << 30 }
    }
}

/// A coverage proof bound to checked finite planar geometry.
#[derive(Debug, Clone)]
pub struct GeometryBoundCoverageArtifact {
    geometry: CoverageGeometry,
    coverage: CoverageSynthesisArtifact,
    digest: [u8; 32],
}

impl GeometryBoundCoverageArtifact {
    /// Bind an existing coverage proof to checked state coordinates.
    pub fn build(
        coverage: CoverageSynthesisArtifact,
        geometry: CoverageGeometry,
        coverage_limits: CoverageSynthesisLimits,
        geometry_limits: CoverageGeometryLimits,
    ) -> Result<Self> {
        coverage.verify(coverage_limits)?;
        geometry.verify(coverage.specification(), geometry_limits)?;
        let mut artifact = Self {
            geometry,
            coverage,
            digest: [0; 32],
        };
        artifact.digest = artifact.compute_digest(coverage_limits)?;
        Ok(artifact)
    }

    /// Checked coordinates in state order.
    pub fn geometry(&self) -> &CoverageGeometry {
        &self.geometry
    }

    /// Nested exact coverage and optimality proof.
    pub fn coverage(&self) -> &CoverageSynthesisArtifact {
        &self.coverage
    }

    /// Content digest of the complete outer payload.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Recheck geometry, coverage, optimality, and the outer binding.
    pub fn verify(
        &self,
        coverage_limits: CoverageSynthesisLimits,
        geometry_limits: CoverageGeometryLimits,
    ) -> Result<()> {
        self.coverage.verify(coverage_limits)?;
        self.geometry
            .verify(self.coverage.specification(), geometry_limits)?;
        if self.compute_digest(coverage_limits)? != self.digest {
            return Err(geometry_error(
                "geometry-bound artifact digest does not match",
            ));
        }
        Ok(())
    }

    /// Encode canonical `HOLOSGEO` version 1 bytes.
    pub fn encode(
        &self,
        coverage_limits: CoverageSynthesisLimits,
        geometry_limits: CoverageGeometryLimits,
        decode_limits: GeometryBoundCoverageDecodeLimits,
    ) -> Result<Vec<u8>> {
        self.verify(coverage_limits, geometry_limits)?;
        let mut output = self.encode_payload(coverage_limits)?;
        output.extend_from_slice(&self.digest);
        if output.len() > decode_limits.max_bytes {
            return Err(geometry_error(
                "geometry-bound artifact exceeds its byte limit",
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSGEO` version 1 bytes.
    pub fn decode(
        bytes: &[u8],
        coverage_limits: CoverageSynthesisLimits,
        geometry_limits: CoverageGeometryLimits,
        decode_limits: GeometryBoundCoverageDecodeLimits,
    ) -> Result<Self> {
        let (payload, digest) = decode_digest(bytes, decode_limits.max_bytes)?;
        let artifact = decode_payload(payload, digest, coverage_limits, geometry_limits)?;
        artifact.verify(coverage_limits, geometry_limits)?;
        Ok(artifact)
    }

    fn encode_payload(&self, coverage_limits: CoverageSynthesisLimits) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        encode_coordinates(&mut output, self.geometry.coordinates())?;
        let coverage = self.coverage.encode(coverage_limits)?;
        put_usize(&mut output, coverage.len(), "coverage artifact byte count")?;
        output.extend_from_slice(&coverage);
        Ok(output)
    }

    fn compute_digest(&self, coverage_limits: CoverageSynthesisLimits) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.encode_payload(coverage_limits)?).into())
    }
}

fn encode_coordinates(output: &mut Vec<u8>, states: &[Vec<PlanarPoint>]) -> Result<()> {
    put_usize(output, states.len(), "geometry state count")?;
    for coordinates in states {
        put_usize(output, coordinates.len(), "geometry vertex count")?;
        for point in coordinates {
            put_u64(output, point.x().to_bits());
            put_u64(output, point.y().to_bits());
        }
    }
    Ok(())
}

fn decode_digest(bytes: &[u8], maximum: usize) -> Result<(&[u8], [u8; 32])> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(geometry_error(
            "geometry-bound artifact is truncated or exceeds its byte limit",
        ));
    }
    let payload_length = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_length]).into();
    let digest: [u8; 32] = bytes[payload_length..]
        .try_into()
        .expect("32-byte geometry-bound digest");
    if digest != expected {
        return Err(geometry_error(
            "geometry-bound artifact digest does not match its bytes",
        ));
    }
    Ok((&bytes[..payload_length], digest))
}

fn decode_payload(
    payload: &[u8],
    digest: [u8; 32],
    coverage_limits: CoverageSynthesisLimits,
    geometry_limits: CoverageGeometryLimits,
) -> Result<GeometryBoundCoverageArtifact> {
    let mut reader = Reader::new(payload);
    decode_prefix(&mut reader)?;
    let coordinates = decode_coordinates(&mut reader, geometry_limits)?;
    let coverage_bytes =
        reader.bounded_usize("coverage artifact byte count", coverage_limits.max_bytes)?;
    let coverage =
        CoverageSynthesisArtifact::decode(reader.take(coverage_bytes)?, coverage_limits)?;
    reader.finish()?;
    let geometry = CoverageGeometry::new(coverage.specification(), coordinates, geometry_limits)?;
    Ok(GeometryBoundCoverageArtifact {
        geometry,
        coverage,
        digest,
    })
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<()> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(geometry_error(
            "geometry-bound artifact envelope version is unsupported",
        ));
    }
    Ok(())
}

fn decode_coordinates(
    reader: &mut Reader<'_>,
    limits: CoverageGeometryLimits,
) -> Result<Vec<Vec<PlanarPoint>>> {
    let state_count = reader.bounded_usize("geometry state count", limits.max_states)?;
    let mut total = 0usize;
    let mut states = Vec::with_capacity(state_count);
    for _ in 0..state_count {
        let count = reader.bounded_usize("geometry vertex count", limits.max_vertices)?;
        total = total
            .checked_add(count)
            .ok_or_else(|| geometry_error("geometry coordinate count overflows"))?;
        if total > limits.max_coordinates {
            return Err(geometry_error(
                "geometry coordinates exceed their total limit",
            ));
        }
        states.push(decode_state_coordinates(reader, count)?);
    }
    Ok(states)
}

fn decode_state_coordinates(reader: &mut Reader<'_>, count: usize) -> Result<Vec<PlanarPoint>> {
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let x = f64::from_bits(reader.u64()?);
        let y = f64::from_bits(reader.u64()?);
        let point = PlanarPoint::new(x, y)?;
        if point.x().to_bits() != x.to_bits() || point.y().to_bits() != y.to_bits() {
            return Err(geometry_error(
                "geometry coordinate encoding is not canonical",
            ));
        }
        points.push(point);
    }
    Ok(points)
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(output: &mut Vec<u8>, value: usize, label: &str) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| geometry_error(format!("{label} does not fit the wire integer")))?;
    put_u64(output, value);
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

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| geometry_error("geometry read position overflows"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| geometry_error("geometry-bound artifact is truncated"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte geometry slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte geometry slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| geometry_error("geometry integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(geometry_error(format!(
                "{label} {value} exceeds its limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn finish(&self) -> Result<()> {
        if self.position != self.bytes.len() {
            return Err(geometry_error(
                "geometry-bound artifact has trailing payload bytes",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CoverageAction, CoverageFence, CoverageLimits, CoverageState, CoverageSynthesisStatus,
        PlanarCoverageModel, SparseDistanceMatrix,
    };

    fn specimen() -> (CoverageSynthesisArtifact, CoverageGeometry) {
        let points = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (1.0, 1.0)]
            .into_iter()
            .map(|(x, y)| PlanarPoint::new(x, y).unwrap())
            .collect::<Vec<_>>();
        let graph = SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 1.0),
                (1, 4, 1.0),
                (2, 4, 1.0),
                (3, 4, 1.0),
            ],
        )
        .unwrap();
        let specification = crate::CoverageSpecification::new(
            5,
            PlanarCoverageModel::new(2.0, 2.0).unwrap(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            Vec::new(),
            0,
            vec![CoverageState::new(0, 0, &graph, (0..4).collect(), 2.0).unwrap()],
            CoverageLimits::default(),
        )
        .unwrap();
        let action = CoverageAction::throughout(4, 1, &specification);
        let geometry = CoverageGeometry::new(
            &specification,
            vec![points],
            CoverageGeometryLimits::default(),
        )
        .unwrap();
        let coverage = CoverageSynthesisArtifact::build(
            specification,
            vec![action],
            1,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(coverage.status(), CoverageSynthesisStatus::Optimal);
        (coverage, geometry)
    }

    #[test]
    fn geometry_bound_artifact_round_trips_and_rejects_mutation() {
        let coverage_limits = CoverageSynthesisLimits::default();
        let geometry_limits = CoverageGeometryLimits::default();
        let decode_limits = GeometryBoundCoverageDecodeLimits::default();
        let (coverage, geometry) = specimen();
        let artifact = GeometryBoundCoverageArtifact::build(
            coverage,
            geometry,
            coverage_limits,
            geometry_limits,
        )
        .unwrap();
        let bytes = artifact
            .encode(coverage_limits, geometry_limits, decode_limits)
            .unwrap();
        let decoded = GeometryBoundCoverageArtifact::decode(
            &bytes,
            coverage_limits,
            geometry_limits,
            decode_limits,
        )
        .unwrap();
        assert_eq!(decoded.geometry().coordinates().len(), 1);
        let independently_checked = holos_tda_check::verify_geometry_bound_coverage(
            &bytes,
            holos_tda_check::ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(independently_checked.vertices, 5);
        assert_eq!(independently_checked.pair_checks, 10);

        let mut changed = bytes;
        changed[20] ^= 1;
        assert!(
            GeometryBoundCoverageArtifact::decode(
                &changed,
                coverage_limits,
                geometry_limits,
                decode_limits,
            )
            .is_err()
        );
    }
}
