use crate::{
    CohomologyRelation, CohomologyRestriction, CohomologySpaceId, SparseDistanceMatrix,
    ZigzagBarcode, ZigzagDirection,
};

/// Resource limits for an affine kinetic schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KineticLimits {
    /// Largest accepted edge trajectory count.
    pub max_edges: usize,
    /// Largest accepted pairwise equality test count.
    pub max_pair_tests: usize,
    /// Largest accepted distinct event count.
    pub max_events: usize,
}

impl Default for KineticLimits {
    fn default() -> Self {
        Self {
            max_edges: 100_000,
            max_pair_tests: 20_000_000,
            max_events: 1_000_000,
        }
    }
}

/// Canonical unordered edge key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KineticEdgeKey {
    /// Lower endpoint.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
}

impl KineticEdgeKey {
    /// Construct a key and sort its endpoints.
    pub fn new(u: usize, v: usize) -> Self {
        Self {
            u: u.min(v),
            v: u.max(v),
        }
    }
}

/// One affine edge weight `intercept + velocity * time`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticEdge {
    /// First endpoint.
    pub u: usize,
    /// Second endpoint.
    pub v: usize,
    /// Weight at time zero.
    pub intercept: f64,
    /// Constant weight derivative.
    pub velocity: f64,
}

impl KineticEdge {
    /// Canonical unordered edge key.
    pub fn key(&self) -> KineticEdgeKey {
        KineticEdgeKey::new(self.u, self.v)
    }
}

/// One isolated change in the affine filtration arrangement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum KineticEventKind {
    /// An edge crosses the requested fixed scale.
    ThresholdCrossing {
        /// Crossing edge.
        edge: KineticEdgeKey,
    },
    /// Two edge weights exchange their strict order.
    EdgeOrderSwap {
        /// First edge in canonical order.
        first: KineticEdgeKey,
        /// Second edge in canonical order.
        second: KineticEdgeKey,
    },
}

/// Adjacent-`f64` enclosure of one kinetic event time.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticEvent {
    /// Best `f64` approximation to the exact rational time.
    pub time: f64,
    /// Lower adjacent-`f64` enclosure endpoint.
    pub lower: f64,
    /// Upper adjacent-`f64` enclosure endpoint.
    pub upper: f64,
    /// Simultaneous changes at this exact time.
    pub kinds: Vec<KineticEventKind>,
}

/// Complete event schedule on one closed time interval.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticSchedule {
    /// First time in the interval.
    pub start: f64,
    /// Last time in the interval.
    pub end: f64,
    /// Isolated events in exact time order.
    pub events: Vec<KineticEvent>,
    /// Edge pairs equal throughout the time interval.
    pub persistent_ties: usize,
}

/// Exact class relation immediately before and after one kinetic event.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticCohomologyEvent {
    /// Certified event time and simultaneous changes.
    pub event: KineticEvent,
    /// Space on the open cell before the event.
    pub before_space: CohomologySpaceId,
    /// Space on the open cell after the event.
    pub after_space: CohomologySpaceId,
    /// Rank before the event.
    pub before_rank: usize,
    /// Rank after the event.
    pub after_rank: usize,
    /// Exact cohomology relation across the event.
    pub relation: CohomologyRelation,
}

/// Position represented by one node in a kinetic cohomology zigzag.
#[derive(Debug, Clone, PartialEq)]
pub enum KineticZigzagNodeKind {
    /// One open time cell between exact events.
    OpenCell {
        /// Sample time inside the open cell.
        sample: f64,
    },
    /// The active complex at one exact event time.
    Event(KineticEvent),
}

/// One canonical cohomology space in a kinetic zigzag.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticZigzagNode {
    /// Open cell or exact event represented by this node.
    pub kind: KineticZigzagNodeKind,
    /// Content identifier of the canonical cohomology space.
    pub space: CohomologySpaceId,
    /// Rank of the cohomology space at this node.
    pub rank: usize,
    /// Active edge count in the flag complex.
    pub active_edges: usize,
}

/// One exact restriction arrow adjacent to a kinetic event.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticZigzagArrow {
    /// Direction relative to the left and right node order.
    pub direction: ZigzagDirection,
    /// Restriction from the event complex to the adjacent open-cell complex.
    pub restriction: CohomologyRestriction,
}

/// Exact fixed-scale cohomology zigzag over a kinetic trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticZigzag {
    /// Cohomology dimension.
    pub dimension: usize,
    /// Fixed filtration scale.
    pub scale: f64,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Edge pairs equal throughout the time interval.
    pub persistent_ties: usize,
    /// Alternating open-cell and exact-event spaces.
    pub nodes: Vec<KineticZigzagNode>,
    /// Exact maps between adjacent nodes.
    pub arrows: Vec<KineticZigzagArrow>,
    /// Interval decomposition of the zigzag module.
    pub barcode: ZigzagBarcode,
}

/// Position represented by one graph in a fixed-scale schedule.
#[derive(Debug, Clone, PartialEq)]
pub enum KineticGraphStateKind {
    /// Exact first time of the closed interval.
    Start,
    /// One open time cell between exact events.
    OpenCell {
        /// Sample time inside the open cell.
        sample: f64,
    },
    /// Active graph at one exact event time.
    Event(KineticEvent),
    /// Exact last time of the closed interval.
    End,
}

/// One active graph in a fixed-scale kinetic schedule.
#[derive(Debug, Clone)]
pub struct KineticGraphState {
    /// Endpoint, open cell, or exact event represented by this state.
    pub kind: KineticGraphStateKind,
    /// Active graph at the represented time or open cell.
    pub graph: SparseDistanceMatrix,
}

/// Fixed-envelope affine edge-weight trajectory.
#[derive(Debug, Clone)]
pub struct KineticFiltration {
    pub(super) vertex_count: usize,
    pub(super) edges: Vec<KineticEdge>,
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) limits: KineticLimits,
}
