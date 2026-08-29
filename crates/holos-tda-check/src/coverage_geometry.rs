//! Coverage proofs bound to planar geometry.

use num_rational::BigRational;
use num_traits::{Signed, Zero};
use sha2::{Digest, Sha256};

use crate::coverage::{CoverageGeometryClaim, verify_coverage_with_geometry_claim};
use crate::{ProofError, ProofLimits, VerifiedCoverage};

const MAGIC: &[u8; 8] = b"HOLOSGEO";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Summary of a checked geometry-bound coverage proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedGeometryBoundCoverage {
    /// Summary of the nested coverage proof.
    pub coverage: VerifiedCoverage,
    /// Number of sensors in every checked state.
    pub vertices: usize,
    /// Number of finite geometry states.
    pub states: usize,
    /// Number of exact vertex-pair distance checks.
    pub pair_checks: usize,
    /// Exact binary64 bits of the broadcast radius.
    pub broadcast_radius_bits: u64,
    /// Exact binary64 bits of the sensing radius.
    pub sensing_radius_bits: u64,
}

/// Return true when bytes start with a geometry-bound coverage envelope.
pub fn is_geometry_bound_coverage(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify planar geometry and the nested `HOLOSCOV` coverage proof.
///
/// Coordinates are exact dyadic rationals from finite binary64 values.
/// The fence must be a simple nondegenerate polygon that contains every sensor.
/// Each state must be the complete Euclidean broadcast-radius graph.
pub fn verify_geometry_bound_coverage(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedGeometryBoundCoverage, ProofError> {
    let payload = checked_payload(bytes, limits.max_bytes)?;
    let mut reader = Reader::new(payload);
    decode_prefix(&mut reader)?;
    let coordinates = decode_coordinates(&mut reader, limits)?;
    let nested_length = reader.bounded_usize("nested coverage byte count", limits.max_bytes)?;
    let nested = reader.take(nested_length)?;
    reader.finish()?;
    let (coverage, claim) = verify_coverage_with_geometry_claim(nested, limits)?;
    let pair_checks = verify_geometry(&coordinates, &claim, limits)?;
    Ok(VerifiedGeometryBoundCoverage {
        coverage,
        vertices: claim.vertex_count,
        states: coordinates.len(),
        pair_checks,
        broadcast_radius_bits: claim.broadcast_radius.to_bits(),
        sensing_radius_bits: claim.sensing_radius.to_bits(),
    })
}

fn checked_payload(bytes: &[u8], maximum: usize) -> Result<&[u8], ProofError> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(error("artifact is truncated or exceeds its byte limit"));
    }
    let payload_length = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_length]).into();
    if bytes[payload_length..] != expected {
        return Err(error("artifact digest does not match its bytes"));
    }
    Ok(&bytes[..payload_length])
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(error("artifact envelope version is unsupported"));
    }
    Ok(())
}

fn decode_coordinates(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<Vec<ExactPoint>>, ProofError> {
    let state_count = reader.bounded_usize("state count", limits.max_snapshots)?;
    let mut total = 0usize;
    let mut states = Vec::with_capacity(state_count);
    for _ in 0..state_count {
        let count = reader.bounded_usize("vertex count", limits.max_vertices)?;
        total = total
            .checked_add(count)
            .ok_or_else(|| error("coordinate count overflows"))?;
        if total > limits.max_references {
            return Err(error("coordinates exceed their total limit"));
        }
        states.push(decode_state_coordinates(reader, count)?);
    }
    Ok(states)
}

fn decode_state_coordinates(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<ExactPoint>, ProofError> {
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let x_bits = reader.u64()?;
        let y_bits = reader.u64()?;
        let x = f64::from_bits(x_bits);
        let y = f64::from_bits(y_bits);
        if !x.is_finite() || !y.is_finite() || x == 0.0 && x_bits != 0 || y == 0.0 && y_bits != 0 {
            return Err(error("coordinate is not a canonical finite binary64 value"));
        }
        points.push(ExactPoint {
            x: rational(x),
            y: rational(y),
        });
    }
    Ok(points)
}

fn verify_geometry(
    coordinates: &[Vec<ExactPoint>],
    claim: &CoverageGeometryClaim,
    limits: ProofLimits,
) -> Result<usize, ProofError> {
    if coordinates.is_empty() || coordinates.len() != claim.state_edges.len() {
        return Err(error("state count differs from the coverage proof"));
    }
    let pair_count = claim
        .vertex_count
        .checked_mul(claim.vertex_count.saturating_sub(1))
        .and_then(|count| count.checked_div(2))
        .ok_or_else(|| error("vertex-pair count overflows"))?;
    let pair_checks = pair_count
        .checked_mul(coordinates.len())
        .ok_or_else(|| error("total vertex-pair count overflows"))?;
    if pair_checks > limits.max_references {
        return Err(error("vertex-pair checks exceed their work limit"));
    }
    for (points, declared_edges) in coordinates.iter().zip(&claim.state_edges) {
        verify_state_geometry(points, declared_edges, claim)?;
    }
    Ok(pair_checks)
}

fn verify_state_geometry(
    points: &[ExactPoint],
    declared_edges: &[(usize, usize)],
    claim: &CoverageGeometryClaim,
) -> Result<(), ProofError> {
    if points.len() != claim.vertex_count {
        return Err(error("state does not have one point per sensor"));
    }
    let polygon = claim
        .fence
        .iter()
        .map(|&vertex| points[vertex].clone())
        .collect::<Vec<_>>();
    validate_polygon(&polygon)?;
    if points
        .iter()
        .any(|point| !point_in_polygon(point, &polygon))
    {
        return Err(error("a sensor lies outside the fence polygon"));
    }
    if euclidean_edges(points, claim.broadcast_radius) != declared_edges {
        return Err(error(
            "a state differs from its complete Euclidean radius graph",
        ));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct ExactPoint {
    x: BigRational,
    y: BigRational,
}

fn validate_polygon(polygon: &[ExactPoint]) -> Result<(), ProofError> {
    if polygon.len() < 3 || has_repeated_point(polygon) || signed_double_area(polygon).is_zero() {
        return Err(error("fence is not a nondegenerate polygon"));
    }
    for left in 0..polygon.len() {
        for right in left + 1..polygon.len() {
            if !segments_are_adjacent(left, right, polygon.len())
                && segments_intersect(
                    &polygon[left],
                    &polygon[(left + 1) % polygon.len()],
                    &polygon[right],
                    &polygon[(right + 1) % polygon.len()],
                )
            {
                return Err(error("fence polygon intersects itself"));
            }
        }
    }
    Ok(())
}

fn has_repeated_point(points: &[ExactPoint]) -> bool {
    points
        .iter()
        .enumerate()
        .any(|(index, point)| points[..index].contains(point))
}

fn signed_double_area(polygon: &[ExactPoint]) -> BigRational {
    let mut area = BigRational::from_integer(0.into());
    for index in 0..polygon.len() {
        let next = (index + 1) % polygon.len();
        area += &polygon[index].x * &polygon[next].y - &polygon[next].x * &polygon[index].y;
    }
    area
}

fn segments_are_adjacent(left: usize, right: usize, count: usize) -> bool {
    left == right || (left + 1) % count == right || (right + 1) % count == left
}

fn segments_intersect(a: &ExactPoint, b: &ExactPoint, c: &ExactPoint, d: &ExactPoint) -> bool {
    let abc = orientation(a, b, c);
    let abd = orientation(a, b, d);
    let cda = orientation(c, d, a);
    let cdb = orientation(c, d, b);
    opposite_sign(&abc, &abd) && opposite_sign(&cda, &cdb)
        || abc.is_zero() && on_segment(c, a, b)
        || abd.is_zero() && on_segment(d, a, b)
        || cda.is_zero() && on_segment(a, c, d)
        || cdb.is_zero() && on_segment(b, c, d)
}

fn orientation(a: &ExactPoint, b: &ExactPoint, c: &ExactPoint) -> BigRational {
    (&b.x - &a.x) * (&c.y - &a.y) - (&b.y - &a.y) * (&c.x - &a.x)
}

fn opposite_sign(left: &BigRational, right: &BigRational) -> bool {
    (left.is_negative() && right.is_positive()) || (left.is_positive() && right.is_negative())
}

fn on_segment(point: &ExactPoint, start: &ExactPoint, end: &ExactPoint) -> bool {
    orientation(start, end, point).is_zero()
        && between(&point.x, &start.x, &end.x)
        && between(&point.y, &start.y, &end.y)
}

fn between(value: &BigRational, left: &BigRational, right: &BigRational) -> bool {
    value >= left.min(right) && value <= left.max(right)
}

fn point_in_polygon(point: &ExactPoint, polygon: &[ExactPoint]) -> bool {
    if polygon
        .iter()
        .enumerate()
        .any(|(index, start)| on_segment(point, start, &polygon[(index + 1) % polygon.len()]))
    {
        return true;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let start = &polygon[index];
        let end = &polygon[(index + 1) % polygon.len()];
        if (start.y > point.y) != (end.y > point.y) {
            let crossing =
                &start.x + (&point.y - &start.y) * (&end.x - &start.x) / (&end.y - &start.y);
            if crossing > point.x {
                inside = !inside;
            }
        }
    }
    inside
}

fn euclidean_edges(points: &[ExactPoint], radius: f64) -> Vec<(usize, usize)> {
    let radius = rational(radius);
    let radius_squared = &radius * &radius;
    let mut edges = Vec::new();
    for u in 0..points.len() {
        for v in u + 1..points.len() {
            if squared_distance(&points[u], &points[v]) <= radius_squared {
                edges.push((u, v));
            }
        }
    }
    edges
}

fn squared_distance(left: &ExactPoint, right: &ExactPoint) -> BigRational {
    let dx = &left.x - &right.x;
    let dy = &left.y - &right.y;
    &dx * &dx + &dy * &dy
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite binary64 value has a rational form")
}

fn error(message: impl Into<String>) -> ProofError {
    ProofError::new(format!("coverage geometry: {}", message.into()))
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| error("read position overflows"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| error("artifact is truncated"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte geometry slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte geometry slice"),
        ))
    }

    fn bounded_usize(&mut self, label: &str, maximum: usize) -> Result<usize, ProofError> {
        let value = usize::try_from(self.u64()?)
            .map_err(|_| error(format!("{label} does not fit usize")))?;
        if value > maximum {
            return Err(error(format!(
                "{label} {value} exceeds its limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn finish(&self) -> Result<(), ProofError> {
        if self.position != self.bytes.len() {
            return Err(error("artifact has trailing payload bytes"));
        }
        Ok(())
    }
}
