use crate::{Error, PointCloudGraph, PointCloudParams, Result, RipsParams, SparseDistanceMatrix};

use super::model::{
    AtlasEvaluation, EdgeKey, EndpointGradient, LineageId, PersistenceAtlas, UpdateMode,
};

/// One coordinate derivative of a point-distance endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct CoordinateDerivative {
    /// Point index.
    pub point: usize,
    /// Coordinate index.
    pub coordinate: usize,
    /// Analytic derivative of the Euclidean distance before `f64` rounding.
    pub value: f64,
}

/// Point-coordinate derivatives for one finite barcode endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct PointEndpointGradient {
    /// Edge whose distance controls the endpoint.
    pub edge: EdgeKey,
    /// Nonzero derivatives for both endpoints of the edge.
    pub terms: Vec<CoordinateDerivative>,
}

/// Point-coordinate sensitivity of one persistent class space.
#[derive(Debug, Clone, PartialEq)]
pub struct PointClassSensitivity {
    /// Atlas lineage.
    pub lineage: LineageId,
    /// Birth derivative. `None` when a distance tie prevents one gradient.
    pub birth: Option<PointEndpointGradient>,
    /// Death derivative. `None` for an essential death or a distance tie.
    pub death: Option<PointEndpointGradient>,
}

/// Exact sparse point-cloud atlas with a conservative coordinate radius.
#[derive(Debug, Clone)]
pub struct PointPersistenceAtlas {
    points: Vec<Vec<f64>>,
    threshold: f64,
    atlas: PersistenceAtlas,
    coordinate_radius: f64,
}

impl PointPersistenceAtlas {
    /// Build a finite-threshold point-cloud atlas.
    pub fn build(points: &[Vec<f64>], params: &RipsParams) -> Result<Self> {
        let threshold = params.threshold.ok_or_else(|| {
            Error::InvalidInput("a point persistence atlas requires an explicit threshold".into())
        })?;
        if !threshold.is_finite() || threshold < 0.0 {
            return Err(Error::InvalidInput(
                "a point persistence atlas requires a finite non-negative threshold".into(),
            ));
        }
        let graph = PointCloudGraph::build(
            points,
            PointCloudParams::new(threshold).with_threads(params.threads),
        )?;
        let atlas = PersistenceAtlas::build(graph.matrix(), params)?;
        let coordinate_radius = coordinate_radius(points, threshold)?;
        Ok(Self {
            points: points.to_vec(),
            threshold,
            atlas,
            coordinate_radius,
        })
    }

    /// Conservative per-point Euclidean radius. Within it, pairwise distance
    /// relations and threshold memberships stay fixed. A zero radius forces a
    /// rebuild for every changed cloud. Pairwise distance overflow sets it to
    /// zero.
    pub fn coordinate_radius(&self) -> f64 {
        self.coordinate_radius
    }

    /// Underlying edge-weight atlas.
    pub fn edge_atlas(&self) -> &PersistenceAtlas {
        &self.atlas
    }

    /// Analytic point-coordinate gradients at the compiled point cloud.
    pub fn sensitivities(&self) -> Vec<PointClassSensitivity> {
        let evaluation = self
            .atlas
            .evaluate(&self.original_graph())
            .expect("point atlas contains its own valid graph");
        evaluation
            .sensitivities
            .into_iter()
            .map(|sensitivity| PointClassSensitivity {
                lineage: sensitivity.lineage,
                birth: point_gradient(&self.points, sensitivity.birth),
                death: point_gradient(&self.points, sensitivity.death),
            })
            .collect()
    }

    /// Evaluate new coordinates when the displacement radius holds.
    pub fn evaluate(&self, points: &[Vec<f64>]) -> Result<AtlasEvaluation> {
        let displacement = point_displacement(&self.points, points)?;
        let unchanged = self
            .points
            .iter()
            .zip(points)
            .all(|(a, b)| a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()));
        if !unchanged && (self.coordinate_radius == 0.0 || displacement >= self.coordinate_radius) {
            return Err(Error::InvalidInput(format!(
                "point displacement {displacement} reaches atlas radius {}",
                self.coordinate_radius
            )));
        }
        let dense = crate::DistanceMatrix::from_points(points)?;
        let triplets: Vec<_> = self
            .atlas
            .topology
            .iter()
            .map(|edge| (edge.u, edge.v, dense.get(edge.u, edge.v)))
            .collect();
        let updated = SparseDistanceMatrix::from_triplets(points.len(), &triplets)?;
        self.atlas.evaluate(&updated)
    }

    /// Reuse the atlas when the displacement is inside the current radius.
    /// Any other valid point set rebuilds it.
    pub fn update(&self, points: &[Vec<f64>]) -> Result<PointAtlasUpdate> {
        if let Ok(evaluation) = self.evaluate(points) {
            return Ok(PointAtlasUpdate {
                atlas: self.clone(),
                evaluation,
                mode: UpdateMode::Reused,
            });
        }
        let mut params = self.atlas.params.clone();
        params.threshold = Some(self.threshold);
        let atlas = Self::build(points, &params)?;
        let evaluation = atlas.evaluate(points)?;
        Ok(PointAtlasUpdate {
            atlas,
            evaluation,
            mode: UpdateMode::Recomputed,
        })
    }

    fn original_graph(&self) -> SparseDistanceMatrix {
        let dense = crate::DistanceMatrix::from_points(&self.points)
            .expect("point atlas contains validated finite points");
        let triplets: Vec<_> = self
            .atlas
            .topology
            .iter()
            .map(|edge| (edge.u, edge.v, dense.get(edge.u, edge.v)))
            .collect();
        SparseDistanceMatrix::from_triplets(self.points.len(), &triplets)
            .expect("point atlas topology is canonical")
    }
}

/// Result of applying a new point cloud to a point atlas.
#[derive(Debug, Clone)]
pub struct PointAtlasUpdate {
    /// Atlas valid at the new coordinates.
    pub atlas: PointPersistenceAtlas,
    /// Exact result at the new coordinates.
    pub evaluation: AtlasEvaluation,
    /// How the atlas was updated.
    pub mode: UpdateMode,
}

fn coordinate_radius(points: &[Vec<f64>], threshold: f64) -> Result<f64> {
    let dense = crate::DistanceMatrix::from_points(points)?;
    let mut distances = Vec::new();
    let mut threshold_gap = f64::INFINITY;
    for v in 1..points.len() {
        for u in 0..v {
            let distance = dense.get(u, v);
            if !distance.is_finite() {
                return Ok(0.0);
            }
            distances.push(distance);
            threshold_gap = threshold_gap.min((distance - threshold).abs());
        }
    }
    distances.sort_by(f64::total_cmp);
    let mut order_gap = f64::INFINITY;
    for pair in distances.windows(2) {
        let gap = pair[1] - pair[0];
        if gap == 0.0 {
            return Ok(0.0);
        }
        order_gap = order_gap.min(gap);
    }
    let radius = (order_gap / 4.0).min(threshold_gap / 2.0);
    Ok(if radius.is_nan() { 0.0 } else { radius })
}

fn point_displacement(original: &[Vec<f64>], updated: &[Vec<f64>]) -> Result<f64> {
    if original.len() != updated.len() {
        return Err(Error::InvalidInput(format!(
            "point count changed from {} to {}",
            original.len(),
            updated.len()
        )));
    }
    let mut maximum = 0.0f64;
    for (index, (a, b)) in original.iter().zip(updated).enumerate() {
        if a.len() != b.len() {
            return Err(Error::InvalidInput(format!(
                "point {index} changed dimension from {} to {}",
                a.len(),
                b.len()
            )));
        }
        for &b in b {
            if !b.is_finite() {
                return Err(Error::InvalidInput(format!(
                    "point {index} has a non-finite coordinate"
                )));
            }
        }
        maximum = maximum.max(scaled_difference_norm(a, b));
    }
    Ok(maximum)
}

fn point_gradient(
    points: &[Vec<f64>],
    gradient: EndpointGradient,
) -> Option<PointEndpointGradient> {
    let EndpointGradient::Edge(edge) = gradient else {
        return None;
    };
    let distance = scaled_difference_norm(&points[edge.u], &points[edge.v]);
    if distance == 0.0 || !distance.is_finite() {
        return None;
    }
    let mut terms = Vec::new();
    for (coordinate, (&a, &b)) in points[edge.u].iter().zip(&points[edge.v]).enumerate() {
        let derivative = (a - b) / distance;
        if derivative != 0.0 {
            terms.push(CoordinateDerivative {
                point: edge.u,
                coordinate,
                value: derivative,
            });
            terms.push(CoordinateDerivative {
                point: edge.v,
                coordinate,
                value: -derivative,
            });
        }
    }
    Some(PointEndpointGradient { edge, terms })
}

pub(crate) fn scaled_difference_norm(a: &[f64], b: &[f64]) -> f64 {
    let mut scale = 0.0f64;
    let mut sum = 1.0f64;
    for (&a, &b) in a.iter().zip(b) {
        let difference = (a - b).abs();
        if difference == 0.0 {
            continue;
        }
        if !difference.is_finite() {
            return f64::INFINITY;
        }
        if scale < difference {
            let ratio = scale / difference;
            sum = 1.0 + sum * ratio * ratio;
            scale = difference;
        } else {
            let ratio = difference / scale;
            sum += ratio * ratio;
        }
    }
    if scale == 0.0 {
        0.0
    } else {
        scale * sum.sqrt()
    }
}
