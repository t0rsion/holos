use num_rational::BigRational;

use crate::{CohomologyLimits, Error, Result, cohomology_relation, cohomology_space};

use super::arithmetic::{midpoint, public_event, rational};
use super::filtration::previous_time;
use super::model::{KineticCohomologyEvent, KineticEventKind, KineticFiltration};

impl KineticFiltration {
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
            let left_boundary = previous_time(&times, position, &start);
            let right_boundary = times.get(position + 1).unwrap_or(&end);
            output.push(self.cohomology_event_at(
                time,
                left_boundary,
                right_boundary,
                &exact.events[time],
                dimension,
                scale,
                &scale_rational,
                modulus,
                limits,
            )?);
        }
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn cohomology_event_at(
        &self,
        time: &BigRational,
        left_boundary: &BigRational,
        right_boundary: &BigRational,
        kinds: &[KineticEventKind],
        dimension: usize,
        scale: f64,
        scale_rational: &BigRational,
        modulus: u32,
        limits: CohomologyLimits,
    ) -> Result<KineticCohomologyEvent> {
        let before_graph = self.active_graph_at(&midpoint(left_boundary, time), scale_rational)?;
        let after_graph = self.active_graph_at(&midpoint(time, right_boundary), scale_rational)?;
        let before = cohomology_space(&before_graph, dimension, scale, modulus, limits)?;
        let after = cohomology_space(&after_graph, dimension, scale, modulus, limits)?;
        let relation = cohomology_relation(&before_graph, &before, &after_graph, &after, limits)?;
        Ok(KineticCohomologyEvent {
            event: public_event(time, kinds.to_vec())?,
            before_space: before.id(),
            after_space: after.id(),
            before_rank: before.rank(),
            after_rank: after.rank(),
            relation,
        })
    }
}
