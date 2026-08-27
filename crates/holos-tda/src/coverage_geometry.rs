//! Exact planar geometry bindings for finite coverage specifications.
//!
//! The ordinary coverage API checks the relative chain and radius inequality,
//! but accepts the domain geometry as a caller declaration. This stricter
//! profile uses the fence sensor coordinates as the polygonal domain boundary.
//! It checks a simple nondegenerate polygon, containment of every sensor, and
//! the complete Euclidean radius graph in each finite state.

mod wire;

use num_rational::BigRational;
use num_traits::{Signed, Zero};

use crate::{CoverageSource, CoverageSpecification, Error, KineticEdgeKey, Result};

pub use wire::{GeometryBoundCoverageArtifact, GeometryBoundCoverageDecodeLimits};

/// One exact binary64 point in the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanarPoint {
    x: f64,
    y: f64,
}

impl PlanarPoint {
    /// Construct a finite point and canonicalize negative zero.
    pub fn new(x: f64, y: f64) -> Result<Self> {
        if !x.is_finite() || !y.is_finite() {
            return Err(geometry_error("planar coordinates must be finite"));
        }
        Ok(Self {
            x: canonical_zero(x),
            y: canonical_zero(y),
        })
    }

    /// Horizontal coordinate.
    pub fn x(self) -> f64 {
        self.x
    }

    /// Vertical coordinate.
    pub fn y(self) -> f64 {
        self.y
    }
}

/// Resource limits for exact geometry binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverageGeometryLimits {
    /// Largest accepted finite state count.
    pub max_states: usize,
    /// Largest accepted vertex count in one state.
    pub max_vertices: usize,
    /// Largest accepted total coordinate count.
    pub max_coordinates: usize,
    /// Largest vertex-pair count checked in one state.
    pub max_pairs_per_state: u64,
}

impl Default for CoverageGeometryLimits {
    fn default() -> Self {
        Self {
            max_states: 4_096,
            max_vertices: 1_000_000,
            max_coordinates: 10_000_000,
            max_pairs_per_state: 100_000_000,
        }
    }
}

/// Exact coordinate binding for every state of a finite coverage problem.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageGeometry {
    coordinates: Vec<Vec<PlanarPoint>>,
}

impl CoverageGeometry {
    /// Bind one coordinate vector to each canonical finite state.
    pub fn new(
        specification: &CoverageSpecification,
        coordinates: Vec<Vec<PlanarPoint>>,
        limits: CoverageGeometryLimits,
    ) -> Result<Self> {
        let geometry = Self { coordinates };
        geometry.verify(specification, limits)?;
        Ok(geometry)
    }

    /// State coordinates in specification order.
    pub fn coordinates(&self) -> &[Vec<PlanarPoint>] {
        &self.coordinates
    }

    /// Recheck polygon geometry and every exact Euclidean radius graph.
    pub fn verify(
        &self,
        specification: &CoverageSpecification,
        limits: CoverageGeometryLimits,
    ) -> Result<()> {
        validate_geometry_scope(specification, &self.coordinates, limits)?;
        for (state, coordinates) in specification.states().iter().zip(&self.coordinates) {
            validate_state_geometry(specification, state.possible_edges(), coordinates, limits)?;
        }
        Ok(())
    }
}

fn validate_geometry_scope(
    specification: &CoverageSpecification,
    coordinates: &[Vec<PlanarPoint>],
    limits: CoverageGeometryLimits,
) -> Result<()> {
    if !matches!(specification.source(), CoverageSource::Finite) {
        return Err(geometry_error(
            "geometry binding accepts finite coverage states only",
        ));
    }
    if specification.states().is_empty()
        || specification.states().len() > limits.max_states
        || coordinates.len() != specification.states().len()
        || specification.vertex_count() > limits.max_vertices
    {
        return Err(geometry_error(
            "geometry state or vertex count is empty, mismatched, or over its limit",
        ));
    }
    let total = coordinates.iter().try_fold(0usize, |total, state| {
        total
            .checked_add(state.len())
            .ok_or_else(|| geometry_error("geometry coordinate count overflows"))
    })?;
    if total > limits.max_coordinates {
        return Err(geometry_error(
            "geometry coordinates exceed their total limit",
        ));
    }
    Ok(())
}

fn validate_state_geometry(
    specification: &CoverageSpecification,
    declared_edges: &[KineticEdgeKey],
    coordinates: &[PlanarPoint],
    limits: CoverageGeometryLimits,
) -> Result<()> {
    if coordinates.len() != specification.vertex_count() {
        return Err(geometry_error(
            "geometry state does not have one point per vertex",
        ));
    }
    let exact = coordinates
        .iter()
        .copied()
        .map(ExactPoint::from)
        .collect::<Vec<_>>();
    let polygon = specification
        .fence()
        .vertices()
        .iter()
        .map(|&vertex| exact[vertex].clone())
        .collect::<Vec<_>>();
    validate_polygon(&polygon)?;
    if exact.iter().any(|point| !point_in_polygon(point, &polygon)) {
        return Err(geometry_error(
            "geometry places a sensor outside the fence polygon",
        ));
    }
    let expected = euclidean_edges(
        &exact,
        specification.model().broadcast_radius(),
        limits.max_pairs_per_state,
    )?;
    if expected != declared_edges {
        return Err(geometry_error(
            "coverage state differs from its complete Euclidean radius graph",
        ));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct ExactPoint {
    x: BigRational,
    y: BigRational,
}

impl From<PlanarPoint> for ExactPoint {
    fn from(point: PlanarPoint) -> Self {
        Self {
            x: rational(point.x),
            y: rational(point.y),
        }
    }
}

fn validate_polygon(polygon: &[ExactPoint]) -> Result<()> {
    if polygon.len() < 3 || has_repeated_point(polygon) || signed_double_area(polygon).is_zero() {
        return Err(geometry_error(
            "fence coordinates do not form a nondegenerate polygon",
        ));
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
                return Err(geometry_error("fence polygon intersects itself"));
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

fn euclidean_edges(
    points: &[ExactPoint],
    broadcast_radius: f64,
    maximum_pairs: u64,
) -> Result<Vec<KineticEdgeKey>> {
    let count = u64::try_from(points.len())
        .map_err(|_| geometry_error("geometry vertex count does not fit u64"))?;
    let pairs = count.saturating_mul(count.saturating_sub(1)) / 2;
    if pairs > maximum_pairs {
        return Err(geometry_error(
            "geometry vertex pairs exceed their state limit",
        ));
    }
    let radius = rational(broadcast_radius);
    let radius_squared = &radius * &radius;
    let mut edges = Vec::new();
    for u in 0..points.len() {
        for v in u + 1..points.len() {
            if squared_distance(&points[u], &points[v]) <= radius_squared {
                edges.push(KineticEdgeKey::new(u, v));
            }
        }
    }
    Ok(edges)
}

fn squared_distance(left: &ExactPoint, right: &ExactPoint) -> BigRational {
    let dx = &left.x - &right.x;
    let dy = &left.y - &right.y;
    &dx * &dx + &dy * &dy
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

fn geometry_error(message: impl Into<String>) -> Error {
    Error::InvalidInput(format!("coverage geometry: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CoverageFence, CoverageLimits, CoverageState, PlanarCoverageModel, SparseDistanceMatrix,
    };

    fn square_points() -> Vec<PlanarPoint> {
        [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (1.0, 1.0)]
            .into_iter()
            .map(|(x, y)| PlanarPoint::new(x, y).unwrap())
            .collect()
    }

    fn specification(points: &[PlanarPoint]) -> CoverageSpecification {
        let exact = points
            .iter()
            .copied()
            .map(ExactPoint::from)
            .collect::<Vec<_>>();
        let edges = euclidean_edges(&exact, 2.0, 100).unwrap();
        let triplets = edges
            .iter()
            .map(|edge| (edge.u, edge.v, 1.0))
            .collect::<Vec<_>>();
        let graph = SparseDistanceMatrix::from_triplets(points.len(), &triplets).unwrap();
        CoverageSpecification::new(
            points.len(),
            PlanarCoverageModel::new(2.0, 2.0).unwrap(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            Vec::new(),
            0,
            vec![CoverageState::new(0, 0, &graph, (0..4).collect(), 2.0).unwrap()],
            CoverageLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn exact_square_geometry_binds_the_radius_graph() {
        let points = square_points();
        let specification = specification(&points);
        CoverageGeometry::new(
            &specification,
            vec![points],
            CoverageGeometryLimits::default(),
        )
        .unwrap();
    }

    #[test]
    fn geometry_rejects_an_outside_sensor_and_a_crossed_fence() {
        let points = square_points();
        let specification = specification(&points);
        let mut outside = points.clone();
        outside[4] = PlanarPoint::new(3.0, 1.0).unwrap();
        assert!(
            CoverageGeometry::new(
                &specification,
                vec![outside],
                CoverageGeometryLimits::default(),
            )
            .is_err()
        );
        let crossed = vec![points[0], points[2], points[1], points[3], points[4]];
        assert!(
            CoverageGeometry::new(
                &specification,
                vec![crossed],
                CoverageGeometryLimits::default(),
            )
            .is_err()
        );
    }
}
