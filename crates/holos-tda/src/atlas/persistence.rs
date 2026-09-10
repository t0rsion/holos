use crate::classes::group_id;
use crate::{
    Bar, Cocycle, Diagram, EdgeKey, Error, ExplainedDiagram, PersistentClassSpace, Result,
    RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
    rips_persistence_with_classes_sparse,
};

use super::model::{
    AtlasEvaluation, AtlasUpdate, ClassSensitivity, EndpointFormula, EndpointGradient,
    EvaluatedClassSpace, PersistenceAtlas, SpaceFormula, TopologyEvent, TopologyEventKind,
    UpdateMode,
};
use super::support::{
    add_space_bars, atlas_digest, checked_threshold, diagram_bits_equal, edge_values,
    evaluate_critical_pair, evaluated_basis, h0_provenance, previous_float, space_formula,
};

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

    fn values_in_region(&self, updated: &SparseDistanceMatrix) -> Result<Vec<f64>> {
        let (values, events) = self.updated_values_and_events(updated);
        if let Some(event) = events.first() {
            return Err(Error::InvalidInput(format!(
                "atlas validity region ended at {:?}",
                event.kind
            )));
        }
        Ok(values)
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

    /// Evaluate H0, H1, class spaces, and edge-weight gradients.
    pub fn evaluate(&self, updated: &SparseDistanceMatrix) -> Result<AtlasEvaluation> {
        let values = self.values_in_region(updated)?;
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
            let (evaluated, sensitivity) =
                self.evaluate_space(space, formula, &values, terminal)?;
            spaces.push(evaluated);
            sensitivities.push(sensitivity);
        }
        let mut diagram = self.h0_diagram(&values);
        add_space_bars(&mut diagram, &spaces);
        diagram.canonicalize();
        Ok(AtlasEvaluation {
            diagram,
            spaces,
            sensitivities,
        })
    }

    fn evaluate_space(
        &self,
        space: &PersistentClassSpace,
        formula: &SpaceFormula,
        values: &[f64],
        terminal: f64,
    ) -> Result<(EvaluatedClassSpace, ClassSensitivity)> {
        let birth = formula.birth.value(&self.topology, values)?;
        let death = formula
            .death
            .as_ref()
            .map(|death| death.value(&self.topology, values))
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
        let basis = evaluated_basis(id, interval, cocycles);
        let critical_pairs = space
            .critical_pairs
            .iter()
            .map(|pair| evaluate_critical_pair(pair, &self.topology, values))
            .collect::<Result<Vec<_>>>()?;
        let evaluated = EvaluatedClassSpace {
            lineage: formula.lineage,
            space: PersistentClassSpace {
                id,
                interval,
                basis,
                critical_pairs,
            },
        };
        let sensitivity = ClassSensitivity {
            lineage: formula.lineage,
            birth: formula.birth.gradient(),
            death: formula
                .death
                .as_ref()
                .map(EndpointFormula::gradient)
                .unwrap_or(EndpointGradient::Essential),
        };
        Ok((evaluated, sensitivity))
    }

    fn h0_diagram(&self, values: &[f64]) -> Diagram {
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
        diagram
    }

    /// Evaluate only the H0 and H1 diagram.
    ///
    /// Use [`Self::evaluate`] when cocycles or sensitivities are required.
    pub fn evaluate_diagram(&self, updated: &SparseDistanceMatrix) -> Result<Diagram> {
        let values = self.values_in_region(updated)?;
        let mut diagram = self.h0_diagram(&values);
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

    /// When the region holds, reuse the atlas. Otherwise report the events
    /// and rebuild at the new weights.
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
