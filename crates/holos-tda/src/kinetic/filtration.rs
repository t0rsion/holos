use std::collections::BTreeMap;

use num_rational::BigRational;
use num_traits::ToPrimitive;

use crate::{Error, Result, SparseDistanceMatrix};

use super::arithmetic::{midpoint, public_event, rational};
use super::model::{
    KineticEdge, KineticEventKind, KineticFiltration, KineticGraphState, KineticGraphStateKind,
    KineticLimits,
};

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
        validate_time_interval(start, end)?;
        if edges.len() > limits.max_edges {
            return Err(Error::InvalidInput(format!(
                "kinetic edge count exceeds the limit {}",
                limits.max_edges
            )));
        }
        for edge in &mut edges {
            validate_kinetic_edge(edge, vertex_count, start, end)?;
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

    /// Materialize every graph needed to decide a fixed-scale claim on `[start, end]`.
    ///
    /// The output includes both closed endpoints, every exact threshold
    /// event, and one sample in each open threshold cell. The graph is
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
        output.push(self.graph_state(KineticGraphStateKind::Start, &start, &scale)?);
        self.push_critical_cells(&events, &event_times, &start, &end, &scale, &mut output)?;
        output.push(self.graph_state(KineticGraphStateKind::End, &end, &scale)?);
        Ok(output)
    }

    fn push_critical_cells(
        &self,
        events: &BTreeMap<BigRational, Vec<KineticEventKind>>,
        event_times: &[BigRational],
        start: &BigRational,
        end: &BigRational,
        scale: &BigRational,
        output: &mut Vec<KineticGraphState>,
    ) -> Result<()> {
        for position in 0..=event_times.len() {
            let left = previous_time(event_times, position, start);
            let right = event_times.get(position).unwrap_or(end);
            let sample = midpoint(left, right);
            let sample_value = rational_to_f64(&sample, "kinetic graph sample")?;
            output.push(self.graph_state(
                KineticGraphStateKind::OpenCell {
                    sample: sample_value,
                },
                &sample,
                scale,
            )?);
            if let Some(time) = event_times.get(position) {
                let kind = KineticGraphStateKind::Event(public_event(time, events[time].clone())?);
                output.push(self.graph_state(kind, time, scale)?);
            }
        }
        Ok(())
    }

    fn graph_state(
        &self,
        kind: KineticGraphStateKind,
        time: &BigRational,
        scale: &BigRational,
    ) -> Result<KineticGraphState> {
        Ok(KineticGraphState {
            kind,
            graph: self.active_graph_at(time, scale)?,
        })
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

    pub(super) fn active_graph_at(
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

fn validate_time_interval(start: f64, end: f64) -> Result<()> {
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Err(Error::InvalidInput(
            "kinetic times must be finite with start below end".into(),
        ));
    }
    Ok(())
}

fn validate_kinetic_edge(
    edge: &mut KineticEdge,
    vertex_count: usize,
    start: f64,
    end: f64,
) -> Result<()> {
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
    Ok(())
}

pub(super) fn previous_time<'a>(
    event_times: &'a [BigRational],
    position: usize,
    start: &'a BigRational,
) -> &'a BigRational {
    position
        .checked_sub(1)
        .and_then(|previous| event_times.get(previous))
        .unwrap_or(start)
}

pub(super) fn rational_to_f64(value: &BigRational, subject: &str) -> Result<f64> {
    value
        .to_f64()
        .ok_or_else(|| Error::InvalidInput(format!("{subject} does not fit f64")))
}
