//! Exact event schedules for affine edge-weight trajectories.
//!
//! Input `f64` values are interpreted as exact dyadic rationals. Threshold
//! crossings and pairwise order swaps are solved over those rationals. Each
//! public event carries the smallest adjacent-`f64` interval found around its
//! exact time.

use std::collections::BTreeMap;

use num_rational::BigRational;
use num_traits::ToPrimitive;

use crate::{
    CohomologyLimits, CohomologyRelation, CohomologyRestriction, CohomologySpace,
    CohomologySpaceId, Error, Result, SparseDistanceMatrix, ZigzagBarcode, ZigzagDirection,
    ZigzagLimits, ZigzagMap, ZigzagModule, ZigzagTerm, cohomology_relation, cohomology_restriction,
    cohomology_space,
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

/// Certified enclosure of one exact kinetic event time.
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
    /// Edge pairs equal throughout the complete interval.
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
    /// Exact restriction relation across the event.
    pub relation: CohomologyRelation,
}

/// Position represented by one node in a kinetic cohomology zigzag.
#[derive(Debug, Clone, PartialEq)]
pub enum KineticZigzagNodeKind {
    /// One open time cell between exact events.
    OpenCell {
        /// A descriptive sample inside the open cell.
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
    /// Cohomology dimension at this node.
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
    /// Edge pairs tied throughout the complete time interval.
    pub persistent_ties: usize,
    /// Alternating open-cell and exact-event spaces.
    pub nodes: Vec<KineticZigzagNode>,
    /// Exact maps between adjacent nodes.
    pub arrows: Vec<KineticZigzagArrow>,
    /// Interval decomposition of the complete finite zigzag module.
    pub barcode: ZigzagBarcode,
}

/// Position represented by one graph in a complete fixed-scale schedule.
#[derive(Debug, Clone, PartialEq)]
pub enum KineticGraphStateKind {
    /// Exact first time of the closed interval.
    Start,
    /// One open time cell between exact events.
    OpenCell {
        /// A descriptive sample inside the open cell.
        sample: f64,
    },
    /// Active graph at one exact event time.
    Event(KineticEvent),
    /// Exact last time of the closed interval.
    End,
}

/// One active graph in a complete fixed-scale kinetic schedule.
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
    vertex_count: usize,
    edges: Vec<KineticEdge>,
    start: f64,
    end: f64,
    limits: KineticLimits,
}

impl KineticFiltration {
    /// Construct and validate one affine trajectory.
    ///
    /// Every listed edge must have a finite non-negative weight throughout
    /// `[start, end]`. An unlisted edge remains absent.
    pub fn new(
        vertex_count: usize,
        mut edges: Vec<KineticEdge>,
        start: f64,
        end: f64,
        limits: KineticLimits,
    ) -> Result<Self> {
        if !start.is_finite() || !end.is_finite() || start >= end {
            return Err(Error::InvalidInput(
                "kinetic times must be finite with start below end".into(),
            ));
        }
        if edges.len() > limits.max_edges {
            return Err(Error::InvalidInput(format!(
                "kinetic edge count exceeds the limit {}",
                limits.max_edges
            )));
        }
        for edge in &mut edges {
            if edge.u == edge.v || edge.u >= vertex_count || edge.v >= vertex_count {
                return Err(Error::InvalidInput(
                    "kinetic edge endpoints are invalid".into(),
                ));
            }
            if edge.u > edge.v {
                std::mem::swap(&mut edge.u, &mut edge.v);
            }
            if !edge.intercept.is_finite() || !edge.velocity.is_finite() {
                return Err(Error::InvalidInput(
                    "kinetic coefficients must be finite".into(),
                ));
            }
            for time in [start, end] {
                let value = edge.intercept + edge.velocity * time;
                if !value.is_finite() || value < 0.0 {
                    return Err(Error::InvalidInput(
                        "kinetic edge weight must stay finite and non-negative".into(),
                    ));
                }
            }
        }
        edges.sort_by_key(KineticEdge::key);
        if edges.windows(2).any(|pair| pair[0].key() == pair[1].key()) {
            return Err(Error::InvalidInput(
                "kinetic trajectory repeats an edge".into(),
            ));
        }
        Ok(Self {
            vertex_count,
            edges,
            start,
            end,
            limits,
        })
    }

    /// Number of vertices in every trajectory graph.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Canonical affine edge list.
    pub fn edges(&self) -> &[KineticEdge] {
        &self.edges
    }

    /// First trajectory time.
    pub fn start(&self) -> f64 {
        self.start
    }

    /// Last trajectory time.
    pub fn end(&self) -> f64 {
        self.end
    }

    /// Materialize the listed graph at one time.
    pub fn graph_at(&self, time: f64) -> Result<SparseDistanceMatrix> {
        if !time.is_finite() || time < self.start || time > self.end {
            return Err(Error::InvalidInput(
                "kinetic evaluation time lies outside the trajectory".into(),
            ));
        }
        let triplets: Vec<_> = self
            .edges
            .iter()
            .map(|edge| (edge.u, edge.v, edge.intercept + edge.velocity * time))
            .collect();
        SparseDistanceMatrix::from_triplets(self.vertex_count, &triplets)
    }

    /// Materialize every graph needed to decide a fixed-scale all-time claim.
    ///
    /// The output includes both closed endpoints, every exact threshold
    /// event, and one sample in each open threshold cell. Its graph is
    /// constant on each open cell.
    pub fn critical_graphs(&self, scale: f64) -> Result<Vec<KineticGraphState>> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(Error::InvalidInput(
                "kinetic graph scale must be finite and non-negative".into(),
            ));
        }
        let scale = rational(scale);
        let events = self.threshold_events(&scale)?;
        let event_times = events.keys().cloned().collect::<Vec<_>>();
        let start = rational(self.start);
        let end = rational(self.end);
        let mut output = Vec::with_capacity(event_times.len() * 2 + 3);
        output.push(KineticGraphState {
            kind: KineticGraphStateKind::Start,
            graph: self.active_graph_at(&start, &scale)?,
        });
        for position in 0..=event_times.len() {
            let left = if position == 0 {
                &start
            } else {
                &event_times[position - 1]
            };
            let right = event_times.get(position).unwrap_or(&end);
            let sample = midpoint(left, right);
            output.push(KineticGraphState {
                kind: KineticGraphStateKind::OpenCell {
                    sample: sample.to_f64().ok_or_else(|| {
                        Error::InvalidInput("kinetic graph sample does not fit f64".into())
                    })?,
                },
                graph: self.active_graph_at(&sample, &scale)?,
            });
            if let Some(time) = event_times.get(position) {
                output.push(KineticGraphState {
                    kind: KineticGraphStateKind::Event(public_event(time, events[time].clone())?),
                    graph: self.active_graph_at(time, &scale)?,
                });
            }
        }
        output.push(KineticGraphState {
            kind: KineticGraphStateKind::End,
            graph: self.active_graph_at(&end, &scale)?,
        });
        Ok(output)
    }

    fn threshold_events(
        &self,
        scale: &BigRational,
    ) -> Result<BTreeMap<BigRational, Vec<KineticEventKind>>> {
        let start = rational(self.start);
        let end = rational(self.end);
        let mut events = BTreeMap::<BigRational, Vec<KineticEventKind>>::new();
        for edge in &self.edges {
            let velocity = rational(edge.velocity);
            if velocity == BigRational::from_integer(0.into()) {
                continue;
            }
            let time = (scale - rational(edge.intercept)) / velocity;
            if start < time && time < end {
                events
                    .entry(time)
                    .or_default()
                    .push(KineticEventKind::ThresholdCrossing { edge: edge.key() });
            }
        }
        if events.len() > self.limits.max_events {
            return Err(Error::InvalidInput(format!(
                "kinetic event count exceeds the limit {}",
                self.limits.max_events
            )));
        }
        Ok(events)
    }

    /// Compute every isolated order event and optional threshold crossing.
    ///
    /// Pass `None` to report only changes in the weak edge order. Endpoint
    /// equalities are interval boundaries and are not repeated as events.
    pub fn events(&self, threshold: Option<f64>) -> Result<KineticSchedule> {
        let threshold = threshold
            .map(|value| {
                if !value.is_finite() || value < 0.0 {
                    Err(Error::InvalidInput(
                        "kinetic threshold must be finite and non-negative".into(),
                    ))
                } else {
                    Ok(rational(value))
                }
            })
            .transpose()?;
        let exact = self.exact_events(threshold.as_ref())?;
        Ok(KineticSchedule {
            start: self.start,
            end: self.end,
            events: exact
                .events
                .iter()
                .map(|(time, kinds)| public_event(time, kinds.clone()))
                .collect::<Result<_>>()?,
            persistent_ties: exact.persistent_ties,
        })
    }

    /// Relate fixed-scale cohomology across every exact kinetic event.
    ///
    /// Each side uses an exact rational point in the adjacent open event cell.
    /// Edge inclusion is decided over exact dyadic rationals before the active
    /// graph is converted to a zero-weight adjacency graph.
    pub fn cohomology_events(
        &self,
        dimension: usize,
        scale: f64,
        modulus: u32,
        limits: CohomologyLimits,
    ) -> Result<Vec<KineticCohomologyEvent>> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(Error::InvalidInput(
                "kinetic cohomology scale must be finite and non-negative".into(),
            ));
        }
        let scale_rational = rational(scale);
        let exact = self.exact_events(Some(&scale_rational))?;
        let start = rational(self.start);
        let end = rational(self.end);
        let times: Vec<_> = exact.events.keys().cloned().collect();
        let mut output = Vec::with_capacity(times.len());
        for (position, time) in times.iter().enumerate() {
            let left_boundary = if position == 0 {
                &start
            } else {
                &times[position - 1]
            };
            let right_boundary = times.get(position + 1).unwrap_or(&end);
            let before_time = midpoint(left_boundary, time);
            let after_time = midpoint(time, right_boundary);
            let before_graph = self.active_graph_at(&before_time, &scale_rational)?;
            let after_graph = self.active_graph_at(&after_time, &scale_rational)?;
            let before = cohomology_space(&before_graph, dimension, scale, modulus, limits)?;
            let after = cohomology_space(&after_graph, dimension, scale, modulus, limits)?;
            let relation =
                cohomology_relation(&before_graph, &before, &after_graph, &after, limits)?;
            output.push(KineticCohomologyEvent {
                event: public_event(time, exact.events[time].clone())?,
                before_space: before.id(),
                after_space: after.id(),
                before_rank: before.rank(),
                after_rank: after.rank(),
                relation,
            });
        }
        Ok(output)
    }

    /// Build and decompose the exact fixed-scale cohomology zigzag.
    ///
    /// Open time cells alternate with exact event complexes. An event complex
    /// contains each edge whose exact weight is at most `scale`. Inclusion of
    /// an adjacent open-cell complex induces the recorded cohomology
    /// restriction. Repeated interval copies remain one class space with a
    /// multiplicity.
    pub fn cohomology_zigzag(
        &self,
        dimension: usize,
        scale: f64,
        modulus: u32,
        cohomology_limits: CohomologyLimits,
        zigzag_limits: ZigzagLimits,
    ) -> Result<KineticZigzag> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(Error::InvalidInput(
                "kinetic zigzag scale must be finite and non-negative".into(),
            ));
        }
        let scale_rational = rational(scale);
        let exact = self.exact_events(Some(&scale_rational))?;
        let event_times = exact.events.keys().cloned().collect::<Vec<_>>();
        let start = rational(self.start);
        let end = rational(self.end);
        let mut graphs = Vec::with_capacity(event_times.len() * 2 + 1);
        let mut kinds = Vec::with_capacity(event_times.len() * 2 + 1);
        for position in 0..=event_times.len() {
            let left = if position == 0 {
                &start
            } else {
                &event_times[position - 1]
            };
            let right = event_times.get(position).unwrap_or(&end);
            let sample = midpoint(left, right);
            graphs.push(self.active_graph_at(&sample, &scale_rational)?);
            kinds.push(KineticZigzagNodeKind::OpenCell {
                sample: sample.to_f64().ok_or_else(|| {
                    Error::InvalidInput("kinetic zigzag sample does not fit f64".into())
                })?,
            });
            if let Some(time) = event_times.get(position) {
                graphs.push(self.active_graph_at(time, &scale_rational)?);
                kinds.push(KineticZigzagNodeKind::Event(public_event(
                    time,
                    exact.events[time].clone(),
                )?));
            }
        }
        let spaces = graphs
            .iter()
            .map(|graph| cohomology_space(graph, dimension, scale, modulus, cohomology_limits))
            .collect::<Result<Vec<_>>>()?;
        let mut arrows = Vec::with_capacity(graphs.len().saturating_sub(1));
        let mut maps = Vec::with_capacity(graphs.len().saturating_sub(1));
        for position in 0..event_times.len() {
            let left = 2 * position;
            let event = left + 1;
            let right = left + 2;
            let left_restriction = cohomology_restriction(
                &graphs[event],
                &spaces[event],
                &graphs[left],
                &spaces[left],
            )?;
            maps.push(zigzag_map(
                ZigzagDirection::Backward,
                &left_restriction,
                &spaces[left],
            )?);
            arrows.push(KineticZigzagArrow {
                direction: ZigzagDirection::Backward,
                restriction: left_restriction,
            });
            let right_restriction = cohomology_restriction(
                &graphs[event],
                &spaces[event],
                &graphs[right],
                &spaces[right],
            )?;
            maps.push(zigzag_map(
                ZigzagDirection::Forward,
                &right_restriction,
                &spaces[right],
            )?);
            arrows.push(KineticZigzagArrow {
                direction: ZigzagDirection::Forward,
                restriction: right_restriction,
            });
        }
        let dimensions = spaces.iter().map(CohomologySpace::rank).collect::<Vec<_>>();
        let module = ZigzagModule::new(modulus, dimensions, maps, zigzag_limits)?;
        let barcode = module.decompose()?;
        let nodes = kinds
            .into_iter()
            .zip(spaces)
            .zip(graphs)
            .map(|((kind, space), graph)| KineticZigzagNode {
                kind,
                space: space.id(),
                rank: space.rank(),
                active_edges: graph.num_edges(),
            })
            .collect();
        Ok(KineticZigzag {
            dimension,
            scale,
            modulus,
            persistent_ties: exact.persistent_ties,
            nodes,
            arrows,
            barcode,
        })
    }

    fn exact_events(&self, threshold: Option<&BigRational>) -> Result<ExactSchedule> {
        let pair_tests = self
            .edges
            .len()
            .checked_mul(self.edges.len().saturating_sub(1))
            .map(|value| value / 2)
            .ok_or_else(|| Error::InvalidInput("kinetic pair count overflows".into()))?;
        if pair_tests > self.limits.max_pair_tests {
            return Err(Error::InvalidInput(format!(
                "kinetic pair count exceeds the limit {}",
                self.limits.max_pair_tests
            )));
        }
        let start = rational(self.start);
        let end = rational(self.end);
        let coefficients: Vec<_> = self
            .edges
            .iter()
            .map(|edge| (rational(edge.intercept), rational(edge.velocity)))
            .collect();
        let mut events: BTreeMap<BigRational, Vec<KineticEventKind>> = BTreeMap::new();
        let mut persistent_ties = 0usize;
        for left in 0..self.edges.len() {
            for right in left + 1..self.edges.len() {
                let numerator = &coefficients[right].0 - &coefficients[left].0;
                let denominator = &coefficients[left].1 - &coefficients[right].1;
                if denominator == BigRational::from_integer(0.into()) {
                    if numerator == BigRational::from_integer(0.into()) {
                        persistent_ties += 1;
                    }
                    continue;
                }
                let time = numerator / denominator;
                if start < time && time < end {
                    events
                        .entry(time)
                        .or_default()
                        .push(KineticEventKind::EdgeOrderSwap {
                            first: self.edges[left].key(),
                            second: self.edges[right].key(),
                        });
                }
            }
        }
        if let Some(threshold) = threshold {
            for (edge, (intercept, velocity)) in self.edges.iter().zip(&coefficients) {
                if velocity == &BigRational::from_integer(0.into()) {
                    continue;
                }
                let time = (threshold - intercept) / velocity;
                if start < time && time < end {
                    events
                        .entry(time)
                        .or_default()
                        .push(KineticEventKind::ThresholdCrossing { edge: edge.key() });
                }
            }
        }
        if events.len() > self.limits.max_events {
            return Err(Error::InvalidInput(format!(
                "kinetic event count exceeds the limit {}",
                self.limits.max_events
            )));
        }
        for kinds in events.values_mut() {
            kinds.sort();
            kinds.dedup();
        }
        Ok(ExactSchedule {
            events,
            persistent_ties,
        })
    }

    fn active_graph_at(
        &self,
        time: &BigRational,
        scale: &BigRational,
    ) -> Result<SparseDistanceMatrix> {
        let triplets: Vec<_> = self
            .edges
            .iter()
            .filter(|edge| rational(edge.intercept) + rational(edge.velocity) * time <= *scale)
            .map(|edge| (edge.u, edge.v, 0.0))
            .collect();
        SparseDistanceMatrix::from_triplets(self.vertex_count, &triplets)
    }
}

fn zigzag_map(
    direction: ZigzagDirection,
    restriction: &CohomologyRestriction,
    target: &CohomologySpace,
) -> Result<ZigzagMap> {
    let positions = target
        .basis()
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect::<BTreeMap<_, _>>();
    let columns = restriction
        .columns
        .iter()
        .map(|column| {
            column
                .image
                .iter()
                .map(|term| {
                    Ok(ZigzagTerm {
                        target: positions.get(&term.class).copied().ok_or_else(|| {
                            Error::InvalidInput(
                                "cohomology restriction names an unknown target class".into(),
                            )
                        })?,
                        coefficient: term.coefficient,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ZigzagMap::new(direction, columns))
}

struct ExactSchedule {
    events: BTreeMap<BigRational, Vec<KineticEventKind>>,
    persistent_ties: usize,
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

fn public_event(time: &BigRational, kinds: Vec<KineticEventKind>) -> Result<KineticEvent> {
    let approximation = time
        .to_f64()
        .ok_or_else(|| Error::InvalidInput("kinetic event does not fit f64".into()))?;
    let mut lower = approximation;
    while rational(lower) > *time {
        lower = next_down(lower);
    }
    let mut upper = approximation;
    while rational(upper) < *time {
        upper = next_up(upper);
    }
    Ok(KineticEvent {
        time: approximation,
        lower,
        upper,
        kinds,
    })
}

fn next_up(value: f64) -> f64 {
    if value == f64::INFINITY {
        return value;
    }
    if value == -0.0 {
        return f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value >= 0.0 { bits + 1 } else { bits - 1 })
}

fn next_down(value: f64) -> f64 {
    if value == f64::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f64::from_bits(1);
    }
    let bits = value.to_bits();
    f64::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_groups_exact_simultaneous_events_and_encloses_roots() {
        let trajectory = KineticFiltration::new(
            4,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.0,
                    velocity: 1.0,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.0,
                    velocity: -1.0,
                },
                KineticEdge {
                    u: 2,
                    v: 3,
                    intercept: 0.5,
                    velocity: 0.0,
                },
            ],
            0.0,
            1.0,
            KineticLimits::default(),
        )
        .unwrap();
        let schedule = trajectory.events(Some(0.5)).unwrap();
        assert_eq!(schedule.events.len(), 1);
        assert_eq!(schedule.events[0].time, 0.5);
        assert_eq!(schedule.events[0].kinds.len(), 5);
        assert!(schedule.events[0].lower <= 0.5 && schedule.events[0].upper >= 0.5);
        assert_eq!(schedule.persistent_ties, 0);
    }

    #[test]
    fn fixed_scale_graphs_ignore_inactive_order_swaps() {
        let trajectory = KineticFiltration::new(
            3,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.8,
                    velocity: 0.4,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.2,
                    velocity: -0.4,
                },
            ],
            0.0,
            1.0,
            KineticLimits::default(),
        )
        .unwrap();
        let graphs = trajectory.critical_graphs(1.5).unwrap();
        assert_eq!(graphs.len(), 3);
        assert!(graphs.iter().all(|state| state.graph.num_edges() == 2));
        assert!(
            graphs
                .iter()
                .all(|state| !matches!(state.kind, KineticGraphStateKind::Event(_)))
        );
    }

    #[test]
    fn nonrepresentable_event_gets_adjacent_float_bounds() {
        let trajectory = KineticFiltration::new(
            3,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.0,
                    velocity: 1.0,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.0,
                    velocity: -2.0,
                },
            ],
            0.0,
            0.5,
            KineticLimits::default(),
        )
        .unwrap();
        let event = &trajectory.events(None).unwrap().events[0];
        let exact = BigRational::new(1.into(), 3.into());
        assert!(rational(event.lower) < exact);
        assert!(rational(event.upper) > exact);
        assert_eq!(next_up(event.lower), event.upper);
    }

    #[test]
    fn cohomology_event_detects_an_h2_birth_and_death_direction() {
        let mut edges = Vec::new();
        for u in 0..6 {
            for v in u + 1..6 {
                if u / 2 != v / 2 {
                    edges.push(KineticEdge {
                        u,
                        v,
                        intercept: 0.0,
                        velocity: 0.0,
                    });
                }
            }
        }
        edges.push(KineticEdge {
            u: 0,
            v: 1,
            intercept: 2.0,
            velocity: -2.0,
        });
        let trajectory =
            KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
        let events = trajectory
            .cohomology_events(2, 1.0, 3, CohomologyLimits::default())
            .unwrap();
        let event = events
            .iter()
            .find(|event| event.before_rank != event.after_rank)
            .unwrap();
        assert_eq!(event.before_rank, 1);
        assert_eq!(event.after_rank, 0);
        assert_eq!(event.relation.relation_rank, 0);
    }

    #[test]
    fn kinetic_zigzag_records_an_exact_h2_death() {
        let mut edges = Vec::new();
        for u in 0..6 {
            for v in u + 1..6 {
                if u / 2 != v / 2 {
                    edges.push(KineticEdge {
                        u,
                        v,
                        intercept: 0.0,
                        velocity: 0.0,
                    });
                }
            }
        }
        edges.push(KineticEdge {
            u: 0,
            v: 1,
            intercept: 2.0,
            velocity: -2.0,
        });
        let trajectory =
            KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
        let zigzag = trajectory
            .cohomology_zigzag(
                2,
                1.0,
                5,
                CohomologyLimits::default(),
                ZigzagLimits::default(),
            )
            .unwrap();
        assert_eq!(
            zigzag
                .nodes
                .iter()
                .map(|node| node.rank)
                .collect::<Vec<_>>(),
            vec![1, 0, 0]
        );
        assert_eq!(zigzag.arrows.len(), 2);
        assert_eq!(zigzag.arrows[0].direction, ZigzagDirection::Backward);
        assert_eq!(zigzag.arrows[1].direction, ZigzagDirection::Forward);
        assert_eq!(zigzag.barcode.intervals.len(), 1);
        assert_eq!(zigzag.barcode.intervals[0].start, 0);
        assert_eq!(zigzag.barcode.intervals[0].end, 0);
    }

    #[test]
    fn inactive_order_swap_carries_one_class_through_the_event() {
        let trajectory = KineticFiltration::new(
            4,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.8,
                    velocity: 0.4,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.2,
                    velocity: -0.4,
                },
                KineticEdge {
                    u: 2,
                    v: 3,
                    intercept: 0.9,
                    velocity: 0.0,
                },
                KineticEdge {
                    u: 0,
                    v: 3,
                    intercept: 0.9,
                    velocity: 0.0,
                },
            ],
            0.0,
            1.0,
            KineticLimits::default(),
        )
        .unwrap();
        let zigzag = trajectory
            .cohomology_zigzag(
                1,
                1.5,
                3,
                CohomologyLimits::default(),
                ZigzagLimits::default(),
            )
            .unwrap();
        assert!(zigzag.nodes.len() >= 3);
        assert!(zigzag.nodes.iter().all(|node| node.rank == 1));
        assert_eq!(zigzag.barcode.intervals.len(), 1);
        assert_eq!(zigzag.barcode.intervals[0].start, 0);
        assert_eq!(zigzag.barcode.intervals[0].end, zigzag.nodes.len() - 1);
    }

    #[test]
    fn simultaneous_death_and_birth_do_not_create_a_false_identity() {
        let mut edges = Vec::new();
        for offset in [0, 4] {
            for (u, v) in [(0, 1), (1, 2), (2, 3), (0, 3)] {
                edges.push(KineticEdge {
                    u: offset + u,
                    v: offset + v,
                    intercept: 0.5,
                    velocity: 0.0,
                });
            }
        }
        edges.push(KineticEdge {
            u: 0,
            v: 2,
            intercept: 2.0,
            velocity: -2.0,
        });
        edges.push(KineticEdge {
            u: 4,
            v: 6,
            intercept: 0.0,
            velocity: 2.0,
        });
        let trajectory =
            KineticFiltration::new(8, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
        let zigzag = trajectory
            .cohomology_zigzag(
                1,
                1.0,
                3,
                CohomologyLimits::default(),
                ZigzagLimits::default(),
            )
            .unwrap();
        let dimensions = zigzag
            .nodes
            .iter()
            .map(|node| node.rank)
            .collect::<Vec<_>>();
        let split = dimensions
            .iter()
            .position(|rank| *rank == 0)
            .expect("the simultaneous event separates both classes");
        assert!(dimensions[..split].iter().all(|rank| *rank == 1));
        assert!(dimensions[split + 1..].iter().all(|rank| *rank == 1));
        assert_eq!(
            zigzag
                .barcode
                .intervals
                .iter()
                .map(|interval| (interval.start, interval.end, interval.multiplicity))
                .collect::<Vec<_>>(),
            vec![(0, split - 1, 1), (split + 1, dimensions.len() - 1, 1)]
        );
    }

    #[test]
    fn limits_and_invalid_trajectories_are_rejected() {
        assert!(
            KineticFiltration::new(
                2,
                vec![KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: -1.0,
                    velocity: 0.0,
                }],
                0.0,
                1.0,
                KineticLimits::default(),
            )
            .is_err()
        );
        let trajectory = KineticFiltration::new(
            3,
            vec![
                KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 0.0,
                    velocity: 1.0,
                },
                KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.0,
                    velocity: -1.0,
                },
            ],
            0.0,
            1.0,
            KineticLimits {
                max_pair_tests: 0,
                ..KineticLimits::default()
            },
        )
        .unwrap();
        assert!(trajectory.events(None).is_err());
    }
}
