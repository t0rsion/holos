use sha2::{Digest, Sha256};

use crate::classes::{basis_class_id, canonical_space_basis, group_id, validate_h1_cocycle};
use crate::{
    Bar, Cocycle, CocycleTerm, CriticalPair, CriticalSimplex, Diagram, PersistentClass,
    PersistentClassSpace, SparseDistanceMatrix,
};

use super::codec::{cocycle_lists_bits_equal, critical_pair_order, diagram_bits_equal};
use super::model::{AtlasArtifact, AtlasArtifactError};

pub(crate) fn check_input_binding(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    threshold: f64,
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    if input.len() != artifact.vertex_count
        || full_graph_digest(input, artifact.threshold) != artifact.input_digest
    {
        return Err(AtlasArtifactError::new(
            "complete input graph binding does not match",
        ));
    }
    if threshold.is_finite()
        && input
            .edges()
            .any(|(_, _, value)| value.is_nan() || value < 0.0)
    {
        return Err(AtlasArtifactError::new("input graph is not canonical"));
    }
    Ok(())
}

pub(crate) fn check_reduction_binding(
    artifact: &AtlasArtifact,
) -> std::result::Result<(), AtlasArtifactError> {
    if artifact.reduction.vertex_count() != artifact.vertex_count
        || artifact.reduction.threshold().map(f64::to_bits) != artifact.threshold.map(f64::to_bits)
        || artifact.reduction.modulus() != artifact.modulus
    {
        return Err(AtlasArtifactError::new(
            "reduction header differs from the atlas header",
        ));
    }
    if !diagram_bits_equal(artifact.reduction.diagram(), &artifact.explained.diagram) {
        return Err(AtlasArtifactError::new(
            "reduction diagram differs from the atlas diagram",
        ));
    }
    Ok(())
}

pub(crate) fn check_diagram_structure(
    diagram: &Diagram,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&canonical, diagram) {
        return Err(AtlasArtifactError::new("bars are not in canonical order"));
    }
    for (index, bar) in diagram.bars.iter().enumerate() {
        check_bar(bar)
            .map_err(|error| AtlasArtifactError::new(format!("bar {index} is invalid: {error}")))?;
    }
    Ok(())
}

pub(crate) fn check_spaces_structure(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut space_bars = Vec::new();
    for (index, space) in artifact.explained.spaces.iter().enumerate() {
        check_space_structure(artifact, input, index, space)?;
        space_bars.extend(std::iter::repeat_n(
            (
                space.interval.birth.to_bits(),
                space.interval.death.to_bits(),
            ),
            space.basis.len(),
        ));
    }
    check_space_intervals(&artifact.explained.diagram, &mut space_bars)
}

pub(crate) fn check_space_structure(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    if space.basis.is_empty() || space.critical_pairs.len() != space.basis.len() {
        return Err(AtlasArtifactError::new(format!(
            "space {index} has inconsistent multiplicity"
        )));
    }
    let cocycles = space
        .basis
        .iter()
        .map(|class| class.cocycle.clone())
        .collect::<Vec<_>>();
    check_canonical_basis(input, artifact.modulus, index, &cocycles)?;
    if group_id(space.interval, artifact.modulus, &cocycles) != space.id {
        return Err(AtlasArtifactError::new(format!(
            "space {index} identifier does not match its basis"
        )));
    }
    check_space_basis(artifact, input, index, space)?;
    check_critical_pairs(artifact.vertex_count, index, space)
}

pub(crate) fn check_canonical_basis(
    input: Option<&SparseDistanceMatrix>,
    modulus: u32,
    index: usize,
    cocycles: &[Cocycle],
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    let canonical = canonical_space_basis(input, modulus, cocycles)
        .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
    if !cocycle_lists_bits_equal(&canonical, cocycles) {
        Err(AtlasArtifactError::new(format!(
            "space {index} basis is not in canonical row-reduced form"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn check_space_basis(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    space_index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    for (basis_index, class) in space.basis.iter().enumerate() {
        check_class_structure(artifact, space_index, basis_index, space, class)?;
        if let Some(input) = input {
            validate_h1_cocycle(input, &class.cocycle)
                .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
            if class.provenance.is_some() {
                class
                    .validate_provenance(input)
                    .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
            }
        }
    }
    Ok(())
}

pub(crate) fn check_class_structure(
    artifact: &AtlasArtifact,
    space_index: usize,
    basis_index: usize,
    space: &PersistentClassSpace,
    class: &PersistentClass,
) -> std::result::Result<(), AtlasArtifactError> {
    if class.group_id != space.id
        || class.basis_index != basis_index
        || class.interval != space.interval
        || class.cocycle.modulus != artifact.modulus
        || basis_class_id(space.id, basis_index, &class.cocycle) != class.id
    {
        return Err(AtlasArtifactError::new(format!(
            "space {space_index} basis {basis_index} is not canonical"
        )));
    }
    check_cocycle_shape(&class.cocycle, artifact.vertex_count).map_err(|error| {
        AtlasArtifactError::new(format!(
            "space {space_index} basis {basis_index} is invalid: {error}"
        ))
    })?;
    if let Some(provenance) = class.provenance.as_ref() {
        provenance
            .validate_metadata(*class.id.as_bytes(), class.interval, &class.cocycle)
            .map_err(|error| {
                AtlasArtifactError::new(format!(
                    "space {space_index} basis {basis_index} provenance is invalid: {error}"
                ))
            })?;
    }
    Ok(())
}

pub(crate) fn check_critical_pairs(
    vertex_count: usize,
    space_index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in &space.critical_pairs {
        check_critical_pair(vertex_count, space_index, space.interval, pair)?;
    }
    if space
        .critical_pairs
        .windows(2)
        .any(|pairs| !critical_pair_order(&pairs[0], &pairs[1]).is_lt())
    {
        return Err(AtlasArtifactError::new(format!(
            "space {space_index} critical pairs are not in canonical order"
        )));
    }
    Ok(())
}

pub(crate) fn check_critical_pair(
    vertex_count: usize,
    space_index: usize,
    interval: Bar,
    pair: &CriticalPair,
) -> std::result::Result<(), AtlasArtifactError> {
    check_critical(&pair.birth, 2, vertex_count, interval.birth)?;
    match (&pair.death, interval.is_essential()) {
        (None, true) => Ok(()),
        (Some(death), false) => check_critical(death, 3, vertex_count, interval.death),
        _ => Err(AtlasArtifactError::new(format!(
            "space {space_index} critical pair has wrong death presence"
        ))),
    }
}

pub(crate) fn check_space_intervals(
    diagram: &Diagram,
    space_bars: &mut Vec<(u64, u64)>,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut h1_bars = diagram
        .in_dim(1)
        .map(|bar| (bar.birth.to_bits(), bar.death.to_bits()))
        .collect::<Vec<_>>();
    h1_bars.sort_unstable();
    space_bars.sort_unstable();
    if h1_bars != *space_bars {
        Err(AtlasArtifactError::new(
            "class-space intervals do not match the H1 diagram",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn check_space_order(
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in spaces.windows(2) {
        let order = pair[0]
            .interval
            .birth
            .total_cmp(&pair[1].interval.birth)
            .then(pair[0].interval.death.total_cmp(&pair[1].interval.death))
            .then(pair[0].id.cmp(&pair[1].id));
        if !order.is_lt() {
            return Err(AtlasArtifactError::new(
                "class spaces are not in strict canonical order",
            ));
        }
    }
    Ok(())
}

pub(crate) fn check_critical_values(
    input: Option<&SparseDistanceMatrix>,
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    for space in spaces {
        for pair in &space.critical_pairs {
            check_critical_value(input, &pair.birth)?;
            if let Some(death) = &pair.death {
                check_critical_value(input, death)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn check_bar(bar: &Bar) -> std::result::Result<(), &'static str> {
    if bar.dim > 1 {
        return Err("dimension exceeds one");
    }
    check_bar_birth(bar)?;
    check_bar_death(bar.death)?;
    if bar.death.is_finite() && bar.death <= bar.birth {
        return Err("finite death does not follow birth");
    }
    Ok(())
}

pub(crate) fn check_bar_birth(bar: &Bar) -> std::result::Result<(), &'static str> {
    if !bar.birth.is_finite() || bar.birth < 0.0 || is_negative_zero(bar.birth) {
        return Err("birth is not a canonical non-negative finite value");
    }
    if bar.dim == 0 && bar.birth.to_bits() != 0 {
        return Err("H0 birth is not positive zero");
    }
    Ok(())
}

pub(crate) fn check_bar_death(death: f64) -> std::result::Result<(), &'static str> {
    if death.is_nan()
        || death < 0.0
        || is_negative_zero(death)
        || (death.is_infinite() && !death.is_sign_positive())
    {
        Err("death is not a canonical non-negative value")
    } else {
        Ok(())
    }
}

pub(crate) fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

pub(crate) fn check_cocycle_shape(
    cocycle: &Cocycle,
    vertex_count: usize,
) -> std::result::Result<(), &'static str> {
    if !cocycle.scale.is_finite() || cocycle.scale < 0.0 || is_negative_zero(cocycle.scale) {
        return Err("scale is not a canonical non-negative finite value");
    }
    if cocycle.terms.is_empty() || cocycle.terms[0].coefficient != 1 {
        return Err("terms are empty or not normalized");
    }
    let mut previous = None;
    for term in &cocycle.terms {
        if !canonical_cocycle_term(term, previous, vertex_count, cocycle.modulus) {
            return Err("terms are not canonical");
        }
        previous = Some((term.u, term.v));
    }
    Ok(())
}

pub(crate) fn canonical_cocycle_term(
    term: &CocycleTerm,
    previous: Option<(usize, usize)>,
    vertex_count: usize,
    modulus: u32,
) -> bool {
    term.u < term.v
        && term.v < vertex_count
        && term.coefficient != 0
        && term.coefficient < modulus
        && previous.is_none_or(|edge| edge < (term.u, term.v))
}

pub(crate) fn checked_threshold(
    threshold: Option<f64>,
) -> std::result::Result<f64, AtlasArtifactError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
        return Err(AtlasArtifactError::new(format!(
            "threshold must be canonical and non-negative, got {value}"
        )));
    }
    Ok(value)
}

pub(crate) fn check_critical(
    simplex: &CriticalSimplex,
    size: usize,
    vertex_count: usize,
    expected_value: f64,
) -> std::result::Result<(), AtlasArtifactError> {
    if simplex.vertices.len() != size
        || simplex
            .vertices
            .iter()
            .any(|&vertex| vertex >= vertex_count)
        || simplex.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || !simplex.value.is_finite()
        || simplex.value < 0.0
        || is_negative_zero(simplex.value)
        || simplex.value.to_bits() != expected_value.to_bits()
    {
        return Err(AtlasArtifactError::new(
            "critical simplex is not canonical for its interval",
        ));
    }
    Ok(())
}

pub(crate) fn check_critical_value(
    input: &SparseDistanceMatrix,
    simplex: &CriticalSimplex,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut value = 0.0f64;
    for i in 0..simplex.vertices.len() {
        for j in i + 1..simplex.vertices.len() {
            let edge = input.get(simplex.vertices[i], simplex.vertices[j]);
            if !edge.is_finite() {
                return Err(AtlasArtifactError::new(
                    "critical simplex contains an absent edge",
                ));
            }
            value = value.max(edge);
        }
    }
    if value.to_bits() != simplex.value.to_bits() {
        return Err(AtlasArtifactError::new(
            "critical simplex value differs from its graph filtration value",
        ));
    }
    Ok(())
}

pub(crate) fn full_graph_digest(input: &SparseDistanceMatrix, threshold: Option<f64>) -> [u8; 32] {
    let edges: Vec<_> = input.edges().collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-persistence-atlas-v1");
    hash.update((input.len() as u64).to_be_bytes());
    hash.update(
        threshold
            .map(f64::to_bits)
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update((edges.len() as u64).to_be_bytes());
    for (u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}
