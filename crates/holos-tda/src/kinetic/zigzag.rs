use std::collections::BTreeMap;

use num_rational::BigRational;

use crate::{
    CohomologyLimits, CohomologyRestriction, CohomologySpace, Error, Result, SparseDistanceMatrix,
    ZigzagDirection, ZigzagLimits, ZigzagMap, ZigzagModule, ZigzagTerm, cohomology_restriction,
    cohomology_space,
};

use super::arithmetic::{midpoint, public_event, rational};
use super::filtration::{previous_time, rational_to_f64};
use super::model::{
    KineticEventKind, KineticFiltration, KineticZigzag, KineticZigzagArrow, KineticZigzagNode,
    KineticZigzagNodeKind,
};

impl KineticFiltration {
    /// Build and decompose the exact fixed-scale cohomology zigzag.
    ///
    /// Open time cells alternate with exact event complexes. An event complex
    /// contains each edge whose exact weight is at most `scale`. Inclusion of
    /// an adjacent open-cell complex induces the recorded cohomology
    /// restriction.
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
        let (graphs, kinds) =
            self.zigzag_graphs(&event_times, &exact.events, &start, &end, &scale_rational)?;
        let spaces = graphs
            .iter()
            .map(|graph| cohomology_space(graph, dimension, scale, modulus, cohomology_limits))
            .collect::<Result<Vec<_>>>()?;
        let (arrows, maps) = zigzag_arrows(&graphs, &spaces, event_times.len())?;
        let dimensions = spaces.iter().map(CohomologySpace::rank).collect::<Vec<_>>();
        let module = ZigzagModule::new(modulus, dimensions, maps, zigzag_limits)?;
        let barcode = module.decompose()?;
        let nodes = zigzag_nodes(kinds, spaces, graphs);
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

    fn zigzag_graphs(
        &self,
        event_times: &[BigRational],
        events: &BTreeMap<BigRational, Vec<KineticEventKind>>,
        start: &BigRational,
        end: &BigRational,
        scale: &BigRational,
    ) -> Result<(Vec<SparseDistanceMatrix>, Vec<KineticZigzagNodeKind>)> {
        let mut graphs = Vec::with_capacity(event_times.len() * 2 + 1);
        let mut kinds = Vec::with_capacity(event_times.len() * 2 + 1);
        for position in 0..=event_times.len() {
            let left = previous_time(event_times, position, start);
            let right = event_times.get(position).unwrap_or(end);
            let sample = midpoint(left, right);
            graphs.push(self.active_graph_at(&sample, scale)?);
            kinds.push(KineticZigzagNodeKind::OpenCell {
                sample: rational_to_f64(&sample, "kinetic zigzag sample")?,
            });
            if let Some(time) = event_times.get(position) {
                graphs.push(self.active_graph_at(time, scale)?);
                kinds.push(KineticZigzagNodeKind::Event(public_event(
                    time,
                    events[time].clone(),
                )?));
            }
        }
        Ok((graphs, kinds))
    }
}

fn zigzag_arrows(
    graphs: &[SparseDistanceMatrix],
    spaces: &[CohomologySpace],
    event_count: usize,
) -> Result<(Vec<KineticZigzagArrow>, Vec<ZigzagMap>)> {
    let mut arrows = Vec::with_capacity(graphs.len().saturating_sub(1));
    let mut maps = Vec::with_capacity(graphs.len().saturating_sub(1));
    for position in 0..event_count {
        let left = 2 * position;
        let event = left + 1;
        let right = left + 2;
        push_zigzag_arrow(
            ZigzagDirection::Backward,
            event,
            left,
            graphs,
            spaces,
            &mut arrows,
            &mut maps,
        )?;
        push_zigzag_arrow(
            ZigzagDirection::Forward,
            event,
            right,
            graphs,
            spaces,
            &mut arrows,
            &mut maps,
        )?;
    }
    Ok((arrows, maps))
}

#[allow(clippy::too_many_arguments)]
fn push_zigzag_arrow(
    direction: ZigzagDirection,
    event: usize,
    adjacent: usize,
    graphs: &[SparseDistanceMatrix],
    spaces: &[CohomologySpace],
    arrows: &mut Vec<KineticZigzagArrow>,
    maps: &mut Vec<ZigzagMap>,
) -> Result<()> {
    let restriction = cohomology_restriction(
        &graphs[event],
        &spaces[event],
        &graphs[adjacent],
        &spaces[adjacent],
    )?;
    maps.push(zigzag_map(direction, &restriction, &spaces[adjacent])?);
    arrows.push(KineticZigzagArrow {
        direction,
        restriction,
    });
    Ok(())
}

fn zigzag_nodes(
    kinds: Vec<KineticZigzagNodeKind>,
    spaces: Vec<CohomologySpace>,
    graphs: Vec<SparseDistanceMatrix>,
) -> Vec<KineticZigzagNode> {
    kinds
        .into_iter()
        .zip(spaces)
        .zip(graphs)
        .map(|((kind, space), graph)| KineticZigzagNode {
            kind,
            space: space.id(),
            rank: space.rank(),
            active_edges: graph.num_edges(),
        })
        .collect()
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
