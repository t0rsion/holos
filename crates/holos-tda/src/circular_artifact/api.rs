//! Circular artifact construction and semantic validation.

use crate::{
    CircularClassTerm, CircularCoordinate, CircularCoordinateContinuation,
    CohomologyContinuationKind, CohomologyLimits, Error, Result, SparseDistanceMatrix,
    cohomology_space,
};

use super::{
    ArtifactContinuation, ArtifactState, CircularArtifactSummary, CircularCoordinateArtifact,
};

impl CircularCoordinateArtifact {
    /// Bind one coordinate to its active flag graph.
    pub fn from_coordinate(
        graph: &SparseDistanceMatrix,
        coordinate: &CircularCoordinate,
    ) -> Result<Self> {
        let state = artifact_state(graph, coordinate.scale, Some(coordinate.clone()));
        validate_coordinate_binding(&state, coordinate, CohomologyLimits::default())?;
        Ok(Self {
            modulus: coordinate.modulus,
            scale: coordinate.scale,
            tolerance: coordinate.tolerance,
            states: vec![state],
            continuation: None,
        })
    }

    /// Bind one conservative continuation to its old and new active graphs.
    pub fn from_continuation(
        old_graph: &SparseDistanceMatrix,
        old_coordinate: &CircularCoordinate,
        new_graph: &SparseDistanceMatrix,
        continuation: &CircularCoordinateContinuation,
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let new_space = validate_continuation_inputs(
            old_graph,
            old_coordinate,
            new_graph,
            continuation,
            limits,
        )?;
        let (target, ambiguity) = continuation_terms(&new_space, continuation)?;
        let old_state = artifact_state(
            old_graph,
            old_coordinate.scale,
            Some(old_coordinate.clone()),
        );
        validate_coordinate_binding(&old_state, old_coordinate, limits)?;
        let new_state = artifact_state(
            new_graph,
            old_coordinate.scale,
            continuation.coordinate.clone(),
        );
        validate_new_state(&new_state, old_coordinate, &target, limits)?;
        Ok(Self {
            modulus: old_coordinate.modulus,
            scale: old_coordinate.scale,
            tolerance: old_coordinate.tolerance,
            states: vec![old_state, new_state],
            continuation: Some(ArtifactContinuation {
                kind: continuation.topology.kind,
                target,
                ambiguity,
            }),
        })
    }

    /// Structural counts without encoding the artifact.
    pub fn summary(&self) -> CircularArtifactSummary {
        CircularArtifactSummary {
            states: self.states.len(),
            coordinates: self
                .states
                .iter()
                .filter(|state| state.coordinate.is_some())
                .count(),
            edges: self.states.iter().map(|state| state.edges.len()).sum(),
            continuation: self.continuation.is_some(),
        }
    }
}

fn artifact_state(
    graph: &SparseDistanceMatrix,
    scale: f64,
    coordinate: Option<CircularCoordinate>,
) -> ArtifactState {
    ArtifactState {
        vertex_count: graph.len(),
        edges: graph
            .edges()
            .filter(|edge| edge.2 <= scale)
            .map(|(u, v, _)| (u, v))
            .collect(),
        coordinate,
    }
}

fn validate_continuation_inputs(
    old_graph: &SparseDistanceMatrix,
    old_coordinate: &CircularCoordinate,
    new_graph: &SparseDistanceMatrix,
    continuation: &CircularCoordinateContinuation,
    limits: CohomologyLimits,
) -> Result<crate::CohomologySpace> {
    if old_graph.len() != new_graph.len() || continuation.topology.old_space != old_coordinate.space
    {
        return Err(Error::InvalidInput(
            "circular artifact continuation belongs to different inputs".into(),
        ));
    }
    let new_space = cohomology_space(
        new_graph,
        1,
        old_coordinate.scale,
        old_coordinate.modulus,
        limits,
    )?;
    if continuation.topology.new_space != new_space.id() {
        return Err(Error::InvalidInput(
            "circular artifact continuation belongs to a different new graph".into(),
        ));
    }
    let expected_coordinate = continuation.topology.kind == CohomologyContinuationKind::Unique;
    if continuation.coordinate.is_some() != expected_coordinate {
        return Err(Error::InvalidInput(
            "circular artifact continuation coordinate does not match its status".into(),
        ));
    }
    Ok(new_space)
}

fn continuation_terms(
    new_space: &crate::CohomologySpace,
    continuation: &CircularCoordinateContinuation,
) -> Result<(Vec<CircularClassTerm>, Vec<Vec<CircularClassTerm>>)> {
    let positions = new_space
        .basis()
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect::<std::collections::BTreeMap<_, _>>();
    let convert = |terms: &[crate::CohomologyRelationTerm]| {
        terms
            .iter()
            .map(|term| continuation_term(&positions, term))
            .collect::<Result<Vec<_>>>()
    };
    let target = convert(&continuation.topology.new)?;
    let ambiguity = continuation
        .topology
        .ambiguity
        .iter()
        .map(|row| convert(row))
        .collect::<Result<Vec<_>>>()?;
    Ok((target, ambiguity))
}

fn continuation_term(
    positions: &std::collections::BTreeMap<crate::CohomologyClassId, usize>,
    term: &crate::CohomologyRelationTerm,
) -> Result<CircularClassTerm> {
    positions
        .get(&term.class)
        .copied()
        .map(|basis_index| CircularClassTerm {
            basis_index,
            coefficient: term.coefficient,
        })
        .ok_or_else(|| {
            Error::InvalidInput("circular continuation names an unknown new class".into())
        })
}

fn validate_new_state(
    state: &ArtifactState,
    old_coordinate: &CircularCoordinate,
    target: &[CircularClassTerm],
    limits: CohomologyLimits,
) -> Result<()> {
    let Some(coordinate) = &state.coordinate else {
        return Ok(());
    };
    if coordinate.modulus != old_coordinate.modulus
        || coordinate.scale.to_bits() != old_coordinate.scale.to_bits()
        || coordinate.tolerance.to_bits() != old_coordinate.tolerance.to_bits()
    {
        return Err(Error::InvalidInput(
            "continued circular coordinate uses different parameters".into(),
        ));
    }
    validate_coordinate_binding(state, coordinate, limits)?;
    let claimed = target
        .iter()
        .map(|term| (term.basis_index, term.coefficient))
        .collect::<Vec<_>>();
    let actual = coordinate
        .class
        .iter()
        .map(|term| (term.basis_index, term.coefficient))
        .collect::<Vec<_>>();
    if claimed != actual {
        return Err(Error::InvalidInput(
            "continued coordinate is not the unique target class".into(),
        ));
    }
    Ok(())
}

fn validate_coordinate_binding(
    state: &ArtifactState,
    coordinate: &CircularCoordinate,
    limits: CohomologyLimits,
) -> Result<()> {
    let graph = SparseDistanceMatrix::from_triplets(
        state.vertex_count,
        &state
            .edges
            .iter()
            .map(|&(u, v)| (u, v, coordinate.scale))
            .collect::<Vec<_>>(),
    )?;
    let space = cohomology_space(&graph, 1, coordinate.scale, coordinate.modulus, limits)?;
    if space.id() != coordinate.space || coordinate.potential.len() != state.vertex_count {
        return Err(Error::InvalidInput(
            "circular coordinate is bound to a different active graph".into(),
        ));
    }
    Ok(())
}
