use std::fmt;

use crate::{Diagram, Error, ExplainedDiagram, PersistentClassSpace, Result, RipsParams};

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
pub struct LineageId(pub(crate) [u8; 32]);

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

/// Gradient of one barcode endpoint with respect to listed edge weights.
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

/// Result evaluated from an atlas.
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

/// Kind of change that ends a local atlas region.
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

/// How an atlas update was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// The certified region held. No persistence reduction ran.
    Reused,
    /// An event invalidated the region. Persistence was recomputed.
    Recomputed,
}

/// Result of applying new weights to an atlas.
#[derive(Debug, Clone)]
pub struct AtlasUpdate {
    /// Atlas valid at the new weights.
    pub atlas: PersistenceAtlas,
    /// Exact result at the new weights.
    pub evaluation: AtlasEvaluation,
    /// How the atlas was updated.
    pub mode: UpdateMode,
    /// Events that forced recomputation. Empty for a reused update.
    pub events: Vec<TopologyEvent>,
}

#[derive(Debug, Clone)]
pub(crate) struct EndpointFormula {
    pub(crate) sources: Vec<EdgeKey>,
}

impl EndpointFormula {
    pub(crate) fn gradient(&self) -> EndpointGradient {
        match self.sources.as_slice() {
            [edge] => EndpointGradient::Edge(*edge),
            edges => EndpointGradient::Tied(edges.to_vec()),
        }
    }

    pub(crate) fn value(&self, topology: &[EdgeKey], values: &[f64]) -> Result<f64> {
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
pub(crate) struct SpaceFormula {
    pub(crate) lineage: LineageId,
    pub(crate) birth: EndpointFormula,
    pub(crate) death: Option<EndpointFormula>,
}

/// An H1 persistence model for one weak edge order.
#[derive(Debug, Clone)]
pub struct PersistenceAtlas {
    pub(crate) vertex_count: usize,
    pub(crate) threshold: Option<f64>,
    pub(crate) topology: Vec<EdgeKey>,
    pub(crate) order: Vec<EdgeKey>,
    pub(crate) order_positions: Vec<usize>,
    pub(crate) values: Vec<f64>,
    pub(crate) original_values: Vec<f64>,
    pub(crate) input_digest: [u8; 32],
    pub(crate) explained: ExplainedDiagram,
    pub(crate) formulas: Vec<SpaceFormula>,
    pub(crate) h0_deaths: Vec<EdgeKey>,
    pub(crate) h0_essential: usize,
    pub(crate) params: RipsParams,
}
