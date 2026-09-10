use std::collections::BTreeMap;

use num_rational::BigRational;

use crate::{Error, Result};

use super::arithmetic::{public_event, rational};
use super::model::{KineticEventKind, KineticFiltration, KineticSchedule};

impl KineticFiltration {
    /// Compute every isolated order event and optional threshold crossing.
    ///
    /// With `None`, the schedule reports only changes in the weak edge order.
    /// Endpoint equalities are interval boundaries and are not repeated as
    /// events.
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

    pub(super) fn exact_events(&self, threshold: Option<&BigRational>) -> Result<ExactSchedule> {
        self.check_pair_limit()?;
        let start = rational(self.start);
        let end = rational(self.end);
        let coefficients: Vec<_> = self
            .edges
            .iter()
            .map(|edge| (rational(edge.intercept), rational(edge.velocity)))
            .collect();
        let mut events: BTreeMap<BigRational, Vec<KineticEventKind>> = BTreeMap::new();
        let persistent_ties = self.add_order_events(&coefficients, &start, &end, &mut events);
        if let Some(threshold) = threshold {
            self.add_threshold_events(threshold, &coefficients, &start, &end, &mut events);
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

    fn check_pair_limit(&self) -> Result<()> {
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
        Ok(())
    }

    fn add_order_events(
        &self,
        coefficients: &[(BigRational, BigRational)],
        start: &BigRational,
        end: &BigRational,
        events: &mut BTreeMap<BigRational, Vec<KineticEventKind>>,
    ) -> usize {
        let mut persistent_ties = 0;
        for left in 0..self.edges.len() {
            for right in left + 1..self.edges.len() {
                persistent_ties +=
                    self.add_order_event(left, right, coefficients, start, end, events);
            }
        }
        persistent_ties
    }

    #[allow(clippy::too_many_arguments)]
    fn add_order_event(
        &self,
        left: usize,
        right: usize,
        coefficients: &[(BigRational, BigRational)],
        start: &BigRational,
        end: &BigRational,
        events: &mut BTreeMap<BigRational, Vec<KineticEventKind>>,
    ) -> usize {
        let numerator = &coefficients[right].0 - &coefficients[left].0;
        let denominator = &coefficients[left].1 - &coefficients[right].1;
        if denominator == BigRational::from_integer(0.into()) {
            return usize::from(numerator == BigRational::from_integer(0.into()));
        }
        let time = numerator / denominator;
        if start < &time && &time < end {
            events
                .entry(time)
                .or_default()
                .push(KineticEventKind::EdgeOrderSwap {
                    first: self.edges[left].key(),
                    second: self.edges[right].key(),
                });
        }
        0
    }

    fn add_threshold_events(
        &self,
        threshold: &BigRational,
        coefficients: &[(BigRational, BigRational)],
        start: &BigRational,
        end: &BigRational,
        events: &mut BTreeMap<BigRational, Vec<KineticEventKind>>,
    ) {
        for (edge, (intercept, velocity)) in self.edges.iter().zip(coefficients) {
            if velocity == &BigRational::from_integer(0.into()) {
                continue;
            }
            let time = (threshold - intercept) / velocity;
            if start < &time && &time < end {
                events
                    .entry(time)
                    .or_default()
                    .push(KineticEventKind::ThresholdCrossing { edge: edge.key() });
            }
        }
    }
}

pub(super) struct ExactSchedule {
    pub(super) events: BTreeMap<BigRational, Vec<KineticEventKind>>,
    pub(super) persistent_ties: usize,
}
