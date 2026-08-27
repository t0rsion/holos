//! Certified local models of H1 persistence under changing edge weights.
//!
//! An atlas fixes the vertex set, edge set, threshold membership, and weak
//! order of all listed edge weights. Within that region, every Rips simplex
//! keeps its filtration position. The persistence pairing, class-space basis,
//! and critical simplices therefore stay fixed. Evaluation updates endpoint
//! values without another persistence reduction.

use std::fmt;

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::classes::{basis_class_id, group_id};
use crate::{
    Bar, Cocycle, CriticalPair, CriticalSimplex, Diagram, Error, ExplainedDiagram, PersistentClass,
    PersistentClassSpace, PointCloudGraph, PointCloudParams, Result, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse, rips_persistence_with_classes_sparse,
};

/// An undirected edge in canonical endpoint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeKey {
    /// Lower endpoint.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
}

impl EdgeKey {
    pub(crate) fn new(u: usize, v: usize) -> Self {
        if u < v {
            Self { u, v }
        } else {
            Self { u: v, v: u }
        }
    }
}

/// Stable identifier of one class space while an atlas is reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineageId([u8; 32]);

impl LineageId {
    /// Identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for LineageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Why an endpoint does or does not have one edge-weight derivative.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EndpointGradient {
    /// The endpoint changes one-for-one with this edge weight.
    Edge(EdgeKey),
    /// Several tied edges control the endpoint.
    Tied(Vec<EdgeKey>),
    /// An essential death has no finite endpoint.
    Essential,
}

/// Endpoint gradients for one persistent class space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSensitivity {
    /// Lineage within the current atlas.
    pub lineage: LineageId,
    /// Birth derivative.
    pub birth: EndpointGradient,
    /// Death derivative.
    pub death: EndpointGradient,
}

/// One evaluated class space and its atlas lineage.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedClassSpace {
    /// Lineage that stays fixed while the atlas contract holds.
    pub lineage: LineageId,
    /// Class space at the evaluated weights.
    pub space: PersistentClassSpace,
}

/// Result evaluated from an atlas without persistence reduction.
#[derive(Debug, Clone)]
pub struct AtlasEvaluation {
    /// Exact H0 and H1 diagram at the supplied weights.
    pub diagram: Diagram,
    /// Evaluated H1 class spaces.
    pub spaces: Vec<EvaluatedClassSpace>,
    /// Endpoint derivatives with respect to independent edge weights.
    pub sensitivities: Vec<ClassSensitivity>,
}

/// A change that invalidates an atlas contract.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyEvent {
    /// Kind of invalidating change.
    pub kind: TopologyEventKind,
    /// First affected edge, when one exists.
    pub first: Option<EdgeKey>,
    /// Second affected edge, when one exists.
    pub second: Option<EdgeKey>,
    /// Previous first-edge weight.
    pub old_first: Option<f64>,
    /// New first-edge weight.
    pub new_first: Option<f64>,
    /// Previous second-edge weight.
    pub old_second: Option<f64>,
    /// New second-edge weight.
    pub new_second: Option<f64>,
}

/// Kind of change that ends a certified local region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TopologyEventKind {
    /// The vertex count changed.
    VertexSetChanged,
    /// A listed edge was added or removed.
    EdgeSetChanged,
    /// An edge crossed the fixed filtration threshold.
    ThresholdCrossing,
    /// Edges that were tied no longer have equal weights.
    EqualitySplit,
    /// Strictly ordered edges became tied.
    EqualityMerge,
    /// Two edges reversed their order.
    OrderSwap,
}

/// Whether an update reused an atlas or performed exact recomputation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// The certified region held, so no persistence reduction ran.
    Reused,
    /// At least one event invalidated the region, so holos recomputed.
    Recomputed,
}

/// Result of applying new weights to an atlas.
#[derive(Debug, Clone)]
pub struct AtlasUpdate {
    /// Atlas valid at the new weights.
    pub atlas: PersistenceAtlas,
    /// Exact result at the new weights.
    pub evaluation: AtlasEvaluation,
    /// Whether reduction was avoided.
    pub mode: UpdateMode,
    /// Events that forced recomputation. Empty for a reused update.
    pub events: Vec<TopologyEvent>,
}

#[derive(Debug, Clone)]
struct EndpointFormula {
    sources: Vec<EdgeKey>,
}

impl EndpointFormula {
    fn gradient(&self) -> EndpointGradient {
        match self.sources.as_slice() {
            [edge] => EndpointGradient::Edge(*edge),
            edges => EndpointGradient::Tied(edges.to_vec()),
        }
    }

    fn value(&self, topology: &[EdgeKey], values: &[f64]) -> Result<f64> {
        let Some(first) = self.sources.first() else {
            return Err(Error::InvalidInput(
                "atlas endpoint has no controlling edge".into(),
            ));
        };
        let position = topology.binary_search(first).map_err(|_| {
            Error::InvalidInput(format!(
                "atlas endpoint edge ({}, {}) is absent",
                first.u, first.v
            ))
        })?;
        let value = values[position];
        for edge in &self.sources[1..] {
            let position = topology.binary_search(edge).map_err(|_| {
                Error::InvalidInput(format!(
                    "atlas endpoint edge ({}, {}) is absent",
                    edge.u, edge.v
                ))
            })?;
            let other = values[position];
            if other.to_bits() != value.to_bits() {
                return Err(Error::InvalidInput(
                    "atlas tied endpoint sources no longer agree".into(),
                ));
            }
        }
        Ok(value)
    }
}

#[derive(Debug, Clone)]
struct SpaceFormula {
    lineage: LineageId,
    birth: EndpointFormula,
    death: Option<EndpointFormula>,
}

/// A certified H1 persistence model for one weak edge order.
#[derive(Debug, Clone)]
pub struct PersistenceAtlas {
    vertex_count: usize,
    threshold: Option<f64>,
    topology: Vec<EdgeKey>,
    order: Vec<EdgeKey>,
    order_positions: Vec<usize>,
    values: Vec<f64>,
    original_values: Vec<f64>,
    input_digest: [u8; 32],
    explained: ExplainedDiagram,
    formulas: Vec<SpaceFormula>,
    h0_deaths: Vec<EdgeKey>,
    h0_essential: usize,
    params: RipsParams,
}

impl PersistenceAtlas {
    /// Build an exact local H1 model of a sparse weighted graph.
    ///
    /// `max_dim` must be one. The selected compute profile runs first. A
    /// fixed explain reduction then produces canonical class spaces and
    /// critical simplices on the caller's graph. Their diagrams must agree.
    pub fn build(input: &SparseDistanceMatrix, params: &RipsParams) -> Result<Self> {
        if params.max_dim != 1 {
            return Err(Error::InvalidInput(
                "a persistence atlas requires max_dim equal to 1".into(),
            ));
        }
        let computed = rips_persistence_sparse(input, params)?;
        let mut fixed = params.clone();
        fixed.collapse_edges = false;
        fixed.factorization = crate::GraphFactorization::Off;
        let explained = rips_persistence_with_classes_sparse(input, &fixed)?;
        if !diagram_bits_equal(&computed, &explained.diagram) {
            return Err(Error::InvalidInput(
                "optimized and atlas reductions returned different diagrams".into(),
            ));
        }
        Self::assemble(input, params, explained, &computed)
    }

    pub(crate) fn from_checked_parts(
        input: &SparseDistanceMatrix,
        modulus: u32,
        threshold: Option<f64>,
        explained: ExplainedDiagram,
    ) -> Result<Self> {
        let mut params = RipsParams::new(1).with_modulus(modulus);
        params.threshold = threshold;
        let expected = explained.diagram.clone();
        Self::assemble(input, &params, explained, &expected)
    }

    fn assemble(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        explained: ExplainedDiagram,
        expected: &Diagram,
    ) -> Result<Self> {
        let threshold_value = checked_threshold(params.threshold)?;
        let mut topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        topology.sort_unstable();
        let value_map = edge_values(input);
        let mut order = topology.clone();
        order.sort_by(|a, b| value_map[a].total_cmp(&value_map[b]).then(a.cmp(b)));
        let order_positions = order
            .iter()
            .map(|edge| topology.binary_search(edge).expect("atlas edge is present"))
            .collect();
        let values = order.iter().map(|edge| value_map[edge]).collect();
        let original_values = topology.iter().map(|edge| value_map[edge]).collect();
        let input_digest = atlas_digest(input.len(), params.threshold, &topology, &value_map);
        let formulas = explained
            .spaces
            .iter()
            .enumerate()
            .map(|(index, space)| space_formula(input, input_digest, index, space))
            .collect::<Result<Vec<_>>>()?;
        let (h0_deaths, h0_essential) = h0_provenance(input, threshold_value);
        let atlas = Self {
            vertex_count: input.len(),
            threshold: params.threshold,
            topology,
            order,
            order_positions,
            values,
            original_values,
            input_digest,
            explained,
            formulas,
            h0_deaths,
            h0_essential,
            params: params.clone(),
        };
        let evaluation = atlas.evaluate(input)?;
        if !diagram_bits_equal(&evaluation.diagram, expected) {
            return Err(Error::InvalidInput(format!(
                "atlas endpoint formulas do not reproduce the input diagram: expected {:?}, got {:?}",
                expected.bars, evaluation.diagram.bars
            )));
        }
        Ok(atlas)
    }

    /// Vertex count fixed by this atlas.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Digest of the initial graph, including every listed edge weight.
    pub fn input_digest(&self) -> &[u8; 32] {
        &self.input_digest
    }

    /// Initial explained result.
    pub fn explained(&self) -> &ExplainedDiagram {
        &self.explained
    }

    /// Return every change that prevents reuse at `updated`.
    pub fn events(&self, updated: &SparseDistanceMatrix) -> Vec<TopologyEvent> {
        self.updated_values_and_events(updated).1
    }

    fn updated_values_and_events(
        &self,
        updated: &SparseDistanceMatrix,
    ) -> (Vec<f64>, Vec<TopologyEvent>) {
        let mut events = Vec::new();
        if updated.len() != self.vertex_count {
            events.push(TopologyEvent {
                kind: TopologyEventKind::VertexSetChanged,
                first: None,
                second: None,
                old_first: Some(self.vertex_count as f64),
                new_first: Some(updated.len() as f64),
                old_second: None,
                new_second: None,
            });
            return (Vec::new(), events);
        }
        let entries: Vec<_> = updated.edges().collect();
        let new_topology: Vec<_> = entries
            .iter()
            .map(|&(u, v, _)| EdgeKey::new(u, v))
            .collect();
        let new_values: Vec<_> = entries.iter().map(|&(_, _, value)| value).collect();
        if new_topology != self.topology {
            let first = self
                .topology
                .iter()
                .find(|edge| new_topology.binary_search(edge).is_err())
                .copied()
                .or_else(|| {
                    new_topology
                        .iter()
                        .find(|edge| self.topology.binary_search(edge).is_err())
                        .copied()
                });
            events.push(TopologyEvent {
                kind: TopologyEventKind::EdgeSetChanged,
                first,
                second: None,
                old_first: first.and_then(|edge| {
                    self.topology
                        .binary_search(&edge)
                        .ok()
                        .map(|position| self.original_values[position])
                }),
                new_first: first.and_then(|edge| {
                    new_topology
                        .binary_search(&edge)
                        .ok()
                        .map(|position| new_values[position])
                }),
                old_second: None,
                new_second: None,
            });
            return (new_values, events);
        }
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        for (position, &edge) in self.topology.iter().enumerate() {
            let old = self.original_values[position];
            let new = new_values[position];
            if (old <= threshold) != (new <= threshold) {
                events.push(TopologyEvent {
                    kind: TopologyEventKind::ThresholdCrossing,
                    first: Some(edge),
                    second: None,
                    old_first: Some(old),
                    new_first: Some(new),
                    old_second: None,
                    new_second: None,
                });
            }
        }
        for index in 1..self.order.len() {
            let first = self.order[index - 1];
            let second = self.order[index];
            let old_first = self.values[index - 1];
            let old_second = self.values[index];
            let new_first = new_values[self.order_positions[index - 1]];
            let new_second = new_values[self.order_positions[index]];
            let kind = if old_first.to_bits() == old_second.to_bits() {
                (new_first.to_bits() != new_second.to_bits())
                    .then_some(TopologyEventKind::EqualitySplit)
            } else if new_first.to_bits() == new_second.to_bits() {
                Some(TopologyEventKind::EqualityMerge)
            } else if new_first > new_second {
                Some(TopologyEventKind::OrderSwap)
            } else {
                None
            };
            if let Some(kind) = kind {
                events.push(TopologyEvent {
                    kind,
                    first: Some(first),
                    second: Some(second),
                    old_first: Some(old_first),
                    new_first: Some(new_first),
                    old_second: Some(old_second),
                    new_second: Some(new_second),
                });
            }
        }
        (new_values, events)
    }

    /// Evaluate H0, H1, class spaces, and edge-weight gradients without
    /// persistence reduction.
    pub fn evaluate(&self, updated: &SparseDistanceMatrix) -> Result<AtlasEvaluation> {
        let (values, events) = self.updated_values_and_events(updated);
        if let Some(event) = events.first() {
            return Err(Error::InvalidInput(format!(
                "atlas validity region ended at {:?}",
                event.kind
            )));
        }
        let terminal = if let Some(threshold) = self.threshold {
            threshold
        } else {
            updated
                .edges()
                .map(|(_, _, value)| value)
                .fold(0.0f64, f64::max)
        };
        let mut spaces = Vec::with_capacity(self.explained.spaces.len());
        let mut sensitivities = Vec::with_capacity(self.explained.spaces.len());
        for (space, formula) in self.explained.spaces.iter().zip(&self.formulas) {
            let birth = formula.birth.value(&self.topology, &values)?;
            let death = formula
                .death
                .as_ref()
                .map(|death| death.value(&self.topology, &values))
                .transpose()?
                .unwrap_or(f64::INFINITY);
            let interval = Bar {
                dim: 1,
                birth,
                death,
            };
            let scale = if death.is_finite() {
                previous_float(death)
            } else {
                terminal
            };
            let cocycles: Vec<_> = space
                .basis
                .iter()
                .map(|class| Cocycle {
                    modulus: class.cocycle.modulus,
                    scale,
                    terms: class.cocycle.terms.clone(),
                })
                .collect();
            let id = group_id(interval, cocycles[0].modulus, &cocycles);
            let basis: Vec<_> = cocycles
                .into_iter()
                .enumerate()
                .map(|(basis_index, cocycle)| PersistentClass {
                    id: basis_class_id(id, basis_index, &cocycle),
                    group_id: id,
                    basis_index,
                    interval,
                    cocycle,
                })
                .collect();
            let critical_pairs = space
                .critical_pairs
                .iter()
                .map(|pair| evaluate_critical_pair(pair, &self.topology, &values))
                .collect::<Result<Vec<_>>>()?;
            spaces.push(EvaluatedClassSpace {
                lineage: formula.lineage,
                space: PersistentClassSpace {
                    id,
                    interval,
                    basis,
                    critical_pairs,
                },
            });
            sensitivities.push(ClassSensitivity {
                lineage: formula.lineage,
                birth: formula.birth.gradient(),
                death: formula
                    .death
                    .as_ref()
                    .map(EndpointFormula::gradient)
                    .unwrap_or(EndpointGradient::Essential),
            });
        }
        let mut diagram = Diagram::default();
        for edge in &self.h0_deaths {
            let position = self
                .topology
                .binary_search(edge)
                .expect("H0 provenance edge is in the atlas topology");
            let death = values[position];
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
        for _ in 0..self.h0_essential {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
        for space in &spaces {
            diagram.bars.extend(std::iter::repeat_n(
                space.space.interval,
                space.space.basis.len(),
            ));
        }
        diagram.canonicalize();
        Ok(AtlasEvaluation {
            diagram,
            spaces,
            sensitivities,
        })
    }

    /// Evaluate only the exact H0 and H1 diagram without persistence
    /// reduction.
    ///
    /// This path avoids rebuilding class-space records and their identifiers.
    /// Use [`Self::evaluate`] when you also need cocycles or sensitivities.
    pub fn evaluate_diagram(&self, updated: &SparseDistanceMatrix) -> Result<Diagram> {
        let (values, events) = self.updated_values_and_events(updated);
        if let Some(event) = events.first() {
            return Err(Error::InvalidInput(format!(
                "atlas validity region ended at {:?}",
                event.kind
            )));
        }
        let mut diagram = Diagram::default();
        for edge in &self.h0_deaths {
            let position = self
                .topology
                .binary_search(edge)
                .expect("H0 provenance edge is in the atlas topology");
            let death = values[position];
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
        for _ in 0..self.h0_essential {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
        for (space, formula) in self.explained.spaces.iter().zip(&self.formulas) {
            let interval = Bar {
                dim: 1,
                birth: formula.birth.value(&self.topology, &values)?,
                death: formula
                    .death
                    .as_ref()
                    .map(|death| death.value(&self.topology, &values))
                    .transpose()?
                    .unwrap_or(f64::INFINITY),
            };
            diagram
                .bars
                .extend(std::iter::repeat_n(interval, space.basis.len()));
        }
        diagram.canonicalize();
        Ok(diagram)
    }

    /// Reuse the atlas when possible. Otherwise, report the events and
    /// perform an exact rebuild at the new weights.
    pub fn update(&self, updated: &SparseDistanceMatrix) -> Result<AtlasUpdate> {
        let events = self.events(updated);
        if events.is_empty() {
            return Ok(AtlasUpdate {
                atlas: self.clone(),
                evaluation: self.evaluate(updated)?,
                mode: UpdateMode::Reused,
                events,
            });
        }
        let atlas = Self::build(updated, &self.params)?;
        let evaluation = atlas.evaluate(updated)?;
        Ok(AtlasUpdate {
            atlas,
            evaluation,
            mode: UpdateMode::Recomputed,
            events,
        })
    }
}

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
    /// Birth derivative. `None` means a distance tie prevents one gradient.
    pub birth: Option<PointEndpointGradient>,
    /// Death derivative. `None` means an essential death or distance tie.
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

    /// Under this per-point Euclidean displacement, all pairwise distance
    /// relations and threshold memberships stay fixed.
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

    /// Evaluate new coordinates without persistence reduction when the
    /// checked displacement radius holds.
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

    /// Recompile after a coordinate event. A point set inside the current
    /// radius reuses the edge atlas; any other valid point set rebuilds it.
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
    /// Whether persistence reduction was avoided.
    pub mode: UpdateMode,
}

fn edge_values(matrix: &SparseDistanceMatrix) -> FxHashMap<EdgeKey, f64> {
    matrix
        .edges()
        .map(|(u, v, value)| (EdgeKey::new(u, v), value))
        .collect()
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(Error::InvalidInput(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn atlas_digest(
    vertex_count: usize,
    threshold: Option<f64>,
    topology: &[EdgeKey],
    values: &FxHashMap<EdgeKey, f64>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-persistence-atlas-v1");
    hash.update((vertex_count as u64).to_be_bytes());
    hash.update(
        threshold
            .map(f64::to_bits)
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update((topology.len() as u64).to_be_bytes());
    for edge in topology {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(values[edge].to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn lineage_id(input_digest: [u8; 32], index: usize, space: &PersistentClassSpace) -> LineageId {
    let mut hash = Sha256::new();
    hash.update(b"holos-class-lineage-v1");
    hash.update(input_digest);
    hash.update((index as u64).to_be_bytes());
    hash.update(space.id.as_bytes());
    LineageId(hash.finalize().into())
}

fn space_formula(
    matrix: &SparseDistanceMatrix,
    digest: [u8; 32],
    index: usize,
    space: &PersistentClassSpace,
) -> Result<SpaceFormula> {
    let mut births = Vec::new();
    let mut deaths = Vec::new();
    for pair in &space.critical_pairs {
        births.extend(critical_sources(matrix, &pair.birth)?);
        if let Some(death) = &pair.death {
            deaths.extend(critical_sources(matrix, death)?);
        }
    }
    births.sort_unstable();
    births.dedup();
    deaths.sort_unstable();
    deaths.dedup();
    if births.is_empty() {
        return Err(Error::InvalidInput(
            "class space has no birth-edge provenance".into(),
        ));
    }
    if space.interval.is_essential() && !deaths.is_empty() {
        return Err(Error::InvalidInput(
            "essential class space has death provenance".into(),
        ));
    }
    if !space.interval.is_essential() && deaths.is_empty() {
        return Err(Error::InvalidInput(
            "finite class space has no death-edge provenance".into(),
        ));
    }
    Ok(SpaceFormula {
        lineage: lineage_id(digest, index, space),
        birth: EndpointFormula { sources: births },
        death: (!space.interval.is_essential()).then_some(EndpointFormula { sources: deaths }),
    })
}

fn critical_sources(
    matrix: &SparseDistanceMatrix,
    simplex: &CriticalSimplex,
) -> Result<Vec<EdgeKey>> {
    let mut sources = Vec::new();
    for right in 1..simplex.vertices.len() {
        for left in 0..right {
            let edge = EdgeKey::new(simplex.vertices[left], simplex.vertices[right]);
            let value = matrix.get(edge.u, edge.v);
            if value.to_bits() == simplex.value.to_bits() {
                sources.push(edge);
            }
        }
    }
    if sources.is_empty() {
        return Err(Error::InvalidInput(
            "critical simplex has no edge at its filtration value".into(),
        ));
    }
    Ok(sources)
}

fn evaluate_critical_pair(
    pair: &CriticalPair,
    topology: &[EdgeKey],
    values: &[f64],
) -> Result<CriticalPair> {
    Ok(CriticalPair {
        birth: evaluate_critical(&pair.birth, topology, values)?,
        death: pair
            .death
            .as_ref()
            .map(|death| evaluate_critical(death, topology, values))
            .transpose()?,
    })
}

fn evaluate_critical(
    simplex: &CriticalSimplex,
    topology: &[EdgeKey],
    values: &[f64],
) -> Result<CriticalSimplex> {
    let mut value = 0.0f64;
    for right in 1..simplex.vertices.len() {
        for left in 0..right {
            let edge = EdgeKey::new(simplex.vertices[left], simplex.vertices[right]);
            let position = topology.binary_search(&edge).map_err(|_| {
                Error::InvalidInput(format!(
                    "critical simplex edge ({}, {}) is absent",
                    edge.u, edge.v
                ))
            })?;
            value = value.max(values[position]);
        }
    }
    Ok(CriticalSimplex {
        vertices: simplex.vertices.clone(),
        value,
    })
}

fn h0_provenance(matrix: &SparseDistanceMatrix, threshold: f64) -> (Vec<EdgeKey>, usize) {
    let mut edges: Vec<_> = matrix
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .map(|(u, v, value)| (value, EdgeKey::new(u, v)))
        .collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut parent: Vec<_> = (0..matrix.len()).collect();
    let mut deaths = Vec::new();
    for (_, edge) in edges {
        let a = dsu_find(&mut parent, edge.u);
        let b = dsu_find(&mut parent, edge.v);
        if a != b {
            parent[b] = a;
            deaths.push(edge);
        }
    }
    let essential = (0..matrix.len())
        .filter(|&vertex| dsu_find(&mut parent, vertex) == vertex)
        .count();
    (deaths, essential)
}

fn dsu_find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != vertex {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

fn previous_float(value: f64) -> f64 {
    debug_assert!(value > 0.0 && value.is_finite());
    f64::from_bits(value.to_bits() - 1)
}

fn coordinate_radius(points: &[Vec<f64>], threshold: f64) -> Result<f64> {
    let dense = crate::DistanceMatrix::from_points(points)?;
    let mut distances = Vec::new();
    let mut threshold_gap = f64::INFINITY;
    for v in 1..points.len() {
        for u in 0..v {
            let distance = dense.get(u, v);
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

fn scaled_difference_norm(a: &[f64], b: &[f64]) -> f64 {
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

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(diagonal: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, diagonal),
                (1, 3, diagonal),
            ],
        )
        .unwrap()
    }

    #[test]
    fn atlas_reuses_a_weak_order_and_recomputes_at_an_event() {
        let input = square(2.0);
        let atlas = PersistenceAtlas::build(&input, &RipsParams::new(1)).unwrap();
        let scaled = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 2.0),
                (1, 2, 2.0),
                (2, 3, 2.0),
                (0, 3, 2.0),
                (0, 2, 5.0),
                (1, 3, 5.0),
            ],
        )
        .unwrap();
        let update = atlas.update(&scaled).unwrap();
        assert_eq!(update.mode, UpdateMode::Reused);
        assert!(update.events.is_empty());
        let h1: Vec<_> = update.evaluation.diagram.in_dim(1).collect();
        assert_eq!(h1.len(), 1);
        assert_eq!((h1[0].birth, h1[0].death), (2.0, 5.0));
        assert_eq!(
            update.evaluation.spaces[0].lineage,
            atlas.evaluate(&input).unwrap().spaces[0].lineage
        );

        let split = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.1),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap();
        let update = atlas.update(&split).unwrap();
        assert_eq!(update.mode, UpdateMode::Recomputed);
        assert!(
            update
                .events
                .iter()
                .any(|event| event.kind == TopologyEventKind::EqualitySplit)
        );
        let exact = rips_persistence_sparse(&split, &RipsParams::new(1)).unwrap();
        assert!(diagram_bits_equal(&update.evaluation.diagram, &exact));
    }

    #[test]
    fn endpoint_gradients_name_unique_and_tied_edges() {
        let atlas = PersistenceAtlas::build(&square(2.0), &RipsParams::new(1)).unwrap();
        let sensitivity = &atlas.evaluate(&square(2.0)).unwrap().sensitivities[0];
        assert!(matches!(sensitivity.birth, EndpointGradient::Edge(_)));
        assert!(matches!(sensitivity.death, EndpointGradient::Edge(_)));
    }

    #[test]
    fn point_radius_reuses_small_changes_and_rejects_its_boundary() {
        let points = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.1],
            vec![1.2, 1.1],
            vec![0.0, 0.9],
        ];
        let params = RipsParams::new(1).with_threshold(2.0);
        let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
        assert!(atlas.coordinate_radius() > 0.0);
        let mut moved = points.clone();
        moved[0][0] += atlas.coordinate_radius() / 4.0;
        let update = atlas.update(&moved).unwrap();
        assert_eq!(update.mode, UpdateMode::Reused);
        let exact_graph = PointCloudGraph::build(&moved, PointCloudParams::new(2.0)).unwrap();
        let exact = rips_persistence_sparse(exact_graph.matrix(), &params).unwrap();
        assert!(diagram_bits_equal(&update.evaluation.diagram, &exact));
    }

    #[test]
    fn random_order_preserving_weights_match_exact_reduction() {
        let mut state = 0xd038_72ab_54f1_c967u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..64 {
            let n = 5 + next() as usize % 5;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 3 {
                        triplets.push((u, v, (1 + next() % 9) as f64));
                    }
                }
            }
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            let params = RipsParams::new(1).with_modulus([2, 3, 5][case % 3]);
            let atlas = PersistenceAtlas::build(&input, &params).unwrap();
            let updated_triplets: Vec<_> = input
                .edges()
                .map(|(u, v, value)| (u, v, value * 3.0 + 0.5))
                .collect();
            let updated = SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
            let evaluated = atlas
                .evaluate(&updated)
                .unwrap_or_else(|error| panic!("case {case}: {error}"));
            let fast = atlas
                .evaluate_diagram(&updated)
                .unwrap_or_else(|error| panic!("case {case}: {error}"));
            let exact = rips_persistence_sparse(&updated, &params).unwrap();
            assert!(
                diagram_bits_equal(&evaluated.diagram, &exact),
                "case {case}"
            );
            assert!(diagram_bits_equal(&fast, &exact), "case {case}");
        }
    }

    #[test]
    fn every_region_boundary_has_an_explicit_event() {
        let distinct = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 1.2),
                (0, 3, 1.4),
                (1, 2, 1.6),
                (1, 3, 1.8),
                (2, 3, 2.0),
            ],
        )
        .unwrap();
        let atlas =
            PersistenceAtlas::build(&distinct, &RipsParams::new(1).with_threshold(1.7)).unwrap();

        let fewer_vertices = SparseDistanceMatrix::from_triplets(3, &[]).unwrap();
        assert_eq!(
            atlas.events(&fewer_vertices)[0].kind,
            TopologyEventKind::VertexSetChanged
        );

        let missing = SparseDistanceMatrix::from_triplets(
            4,
            &distinct
                .edges()
                .filter(|&(u, v, _)| (u, v) != (2, 3))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            atlas.events(&missing)[0].kind,
            TopologyEventKind::EdgeSetChanged
        );

        let changed = |replacement: (usize, usize, f64)| {
            SparseDistanceMatrix::from_triplets(
                4,
                &distinct
                    .edges()
                    .map(|(u, v, value)| {
                        if (u, v) == (replacement.0, replacement.1) {
                            replacement
                        } else {
                            (u, v, value)
                        }
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let crossing = changed((1, 2, 1.75));
        assert!(
            atlas
                .events(&crossing)
                .iter()
                .any(|event| event.kind == TopologyEventKind::ThresholdCrossing)
        );
        let merge = changed((0, 2, 1.0));
        assert!(
            atlas
                .events(&merge)
                .iter()
                .any(|event| event.kind == TopologyEventKind::EqualityMerge)
        );
        let swap = changed((0, 2, 0.9));
        assert!(
            atlas
                .events(&swap)
                .iter()
                .any(|event| event.kind == TopologyEventKind::OrderSwap)
        );

        let tied = PersistenceAtlas::build(&square(2.0), &RipsParams::new(1)).unwrap();
        let split = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.01),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap();
        assert!(
            tied.events(&split)
                .iter()
                .any(|event| event.kind == TopologyEventKind::EqualitySplit)
        );
    }

    #[test]
    fn edge_endpoint_gradients_match_finite_differences() {
        let input = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 2.0),
                (0, 3, 1.1),
                (1, 2, 1.2),
                (1, 3, 2.1),
                (2, 3, 1.3),
            ],
        )
        .unwrap();
        let atlas = PersistenceAtlas::build(&input, &RipsParams::new(1)).unwrap();
        let original = atlas.evaluate(&input).unwrap();
        let sensitivity = &original.sensitivities[0];
        for (gradient, birth) in [(&sensitivity.birth, true), (&sensitivity.death, false)] {
            let EndpointGradient::Edge(source) = gradient else {
                panic!("test graph must have unique endpoint sources");
            };
            let epsilon = 1e-7;
            let changed: Vec<_> = input
                .edges()
                .map(|(u, v, value)| {
                    if (u, v) == (source.u, source.v) {
                        (u, v, value + epsilon)
                    } else {
                        (u, v, value)
                    }
                })
                .collect();
            let changed = SparseDistanceMatrix::from_triplets(4, &changed).unwrap();
            let evaluated = atlas.evaluate(&changed).unwrap();
            let old = original.spaces[0].space.interval;
            let new = evaluated.spaces[0].space.interval;
            let difference = if birth {
                new.birth - old.birth
            } else {
                new.death - old.death
            };
            assert!((difference / epsilon - 1.0).abs() < 1e-8);
        }
    }

    #[test]
    fn point_coordinate_gradients_match_finite_differences() {
        let points = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.1],
            vec![1.2, 1.1],
            vec![0.0, 0.9],
        ];
        let params = RipsParams::new(1).with_threshold(2.0);
        let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
        let original = atlas.evaluate(&points).unwrap();
        let sensitivity = atlas.sensitivities().remove(0);
        for (gradient, birth) in [
            (sensitivity.birth.unwrap(), true),
            (sensitivity.death.unwrap(), false),
        ] {
            let term = &gradient.terms[0];
            let epsilon = atlas.coordinate_radius().min(1e-5) / 100.0;
            let mut changed = points.clone();
            changed[term.point][term.coordinate] += epsilon;
            let evaluated = atlas.evaluate(&changed).unwrap();
            let old = original.spaces[0].space.interval;
            let new = evaluated.spaces[0].space.interval;
            let difference = if birth {
                new.birth - old.birth
            } else {
                new.death - old.death
            };
            assert!((difference / epsilon - term.value).abs() < 1e-5);
        }
    }

    #[test]
    fn random_point_trajectories_inside_the_radius_match_exact_reduction() {
        let mut state = 0x3a11_8e4d_90c7_526bu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..32 {
            let points: Vec<Vec<f64>> = (0..7)
                .map(|point| {
                    (0..3)
                        .map(|coordinate| {
                            (next() % 10_000) as f64 / 997.0
                                + point as f64 * 1e-4
                                + coordinate as f64 * 1e-6
                        })
                        .collect()
                })
                .collect();
            let threshold = 20.0;
            let params = RipsParams::new(1)
                .with_threshold(threshold)
                .with_modulus([2, 3, 5][case % 3]);
            let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
            if atlas.coordinate_radius() == 0.0 {
                continue;
            }
            let shift = atlas.coordinate_radius() / 16.0;
            let mut changed = points.clone();
            for point in &mut changed {
                for value in point {
                    *value += if next() & 1 == 0 { shift } else { -shift };
                }
            }
            let evaluated = atlas
                .evaluate(&changed)
                .unwrap_or_else(|error| panic!("case {case}: {error}"));
            let exact_graph =
                PointCloudGraph::build(&changed, PointCloudParams::new(threshold)).unwrap();
            let exact = rips_persistence_sparse(exact_graph.matrix(), &params).unwrap();
            assert!(
                diagram_bits_equal(&evaluated.diagram, &exact),
                "case {case}"
            );
        }
    }

    #[test]
    fn point_displacement_uses_a_scaled_norm() {
        let norm = scaled_difference_norm(&[0.0, 0.0], &[1e200, -1e200]);
        assert!(norm.is_finite());
        assert!((norm / 1e200 - 2.0f64.sqrt()).abs() < 1e-15);
    }
}
