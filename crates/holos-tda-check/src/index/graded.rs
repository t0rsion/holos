use std::collections::BTreeMap;

use crate::{
    Graph, ProofBar, ProofColumn, ProofError, SparseColumn, canonicalize_diagram, check_matrix,
    checked_threshold, diagrams_equal,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GradedSimplexKey(Vec<usize>);

#[derive(Clone)]
struct GradedSimplex {
    key: GradedSimplexKey,
    value: f64,
}

struct GradedComplex {
    simplices: Vec<Vec<GradedSimplex>>,
    rows: Vec<BTreeMap<GradedSimplexKey, usize>>,
}

impl GradedComplex {
    fn build(
        graph: &Graph,
        threshold: Option<f64>,
        expected_columns: &[Vec<ProofColumn>],
    ) -> Result<Self, ProofError> {
        let threshold = checked_threshold(threshold)?;
        let vertices: Vec<GradedSimplex> = (0..graph.vertex_count)
            .map(|vertex| GradedSimplex {
                key: GradedSimplexKey(vec![vertex]),
                value: 0.0,
            })
            .collect();
        let mut simplices = vec![vertices];
        for (offset, expected) in expected_columns.iter().enumerate() {
            let dimension = offset + 1;
            let next = enumerate_graded_dimension(
                graph,
                threshold,
                dimension,
                &simplices[dimension - 1],
                expected.len(),
            )?;
            simplices.push(next);
        }
        let rows = simplices
            .iter()
            .map(|dimension| {
                dimension
                    .iter()
                    .enumerate()
                    .map(|(position, simplex)| (simplex.key.clone(), position))
                    .collect()
            })
            .collect();
        Ok(Self { simplices, rows })
    }

    fn boundaries(&self, dimension: usize, modulus: u32) -> Result<Vec<SparseColumn>, ProofError> {
        let modulus = modulus as u64;
        self.simplices[dimension]
            .iter()
            .map(|simplex| {
                let mut column = SparseColumn::default();
                for removed in 0..simplex.key.0.len() {
                    let mut face = simplex.key.0.clone();
                    face.remove(removed);
                    let row = self.rows[dimension - 1]
                        .get(&GradedSimplexKey(face))
                        .copied()
                        .ok_or_else(|| ProofError::new("simplex boundary omits a face"))?;
                    column.insert(row, if removed % 2 == 0 { 1 } else { modulus - 1 });
                }
                Ok(column)
            })
            .collect()
    }
}

fn enumerate_graded_dimension(
    graph: &Graph,
    threshold: f64,
    dimension: usize,
    previous: &[GradedSimplex],
    expected: usize,
) -> Result<Vec<GradedSimplex>, ProofError> {
    let mut next = Vec::new();
    for simplex in previous {
        let start = simplex.key.0.last().copied().unwrap_or(0) + 1;
        for vertex in start..graph.vertex_count {
            if let Some(cofacet) = graded_cofacet(graph, threshold, simplex, vertex) {
                if next.len() == expected {
                    return Err(ProofError::new(format!(
                        "dimension {dimension} simplex count exceeds the proof"
                    )));
                }
                next.push(cofacet);
            }
        }
    }
    if next.len() != expected {
        return Err(ProofError::new(format!(
            "dimension {dimension} has {} simplices but the proof records {expected} columns",
            next.len()
        )));
    }
    next.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| right.key.0.iter().rev().cmp(left.key.0.iter().rev()))
    });
    Ok(next)
}

fn graded_cofacet(
    graph: &Graph,
    threshold: f64,
    simplex: &GradedSimplex,
    vertex: usize,
) -> Option<GradedSimplex> {
    let mut value = simplex.value;
    for &member in &simplex.key.0 {
        let edge = graph.get(member, vertex);
        if !edge.is_finite() || edge > threshold {
            return None;
        }
        value = value.max(edge);
    }
    let mut key = simplex.key.0.clone();
    key.push(vertex);
    Some(GradedSimplex {
        key: GradedSimplexKey(key),
        value,
    })
}

pub(super) struct CheckedGradedReduction {
    pub(super) diagram: Vec<ProofBar>,
}

pub(super) fn check_graded_reduction(
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    max_dim: usize,
    columns: &[Vec<ProofColumn>],
) -> Result<CheckedGradedReduction, ProofError> {
    if columns.len() != max_dim + 1 {
        return Err(ProofError::new(
            "materialized interface has the wrong boundary count",
        ));
    }
    let complex = GradedComplex::build(graph, threshold, columns)?;
    let reduced = reduce_graded_complex(&complex, columns, modulus, max_dim)?;
    let diagram = graded_reduction_diagram(&complex, &reduced, graph.vertex_count, max_dim);
    Ok(CheckedGradedReduction { diagram })
}

fn reduce_graded_complex(
    complex: &GradedComplex,
    columns: &[Vec<ProofColumn>],
    modulus: u32,
    max_dim: usize,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let mut reduced = Vec::with_capacity(columns.len());
    for dimension in 1..=max_dim + 1 {
        reduced.push(check_matrix(
            &complex.boundaries(dimension, modulus)?,
            &columns[dimension - 1],
            modulus,
            &format!("dimension {dimension}"),
        )?);
    }
    Ok(reduced)
}

fn graded_reduction_diagram(
    complex: &GradedComplex,
    reduced: &[Vec<SparseColumn>],
    vertex_count: usize,
    max_dim: usize,
) -> Vec<ProofBar> {
    let mut diagram = Vec::new();
    for homology_dimension in 0..=max_dim {
        append_graded_bars(
            &mut diagram,
            complex,
            reduced,
            vertex_count,
            homology_dimension,
        );
    }
    canonicalize_diagram(&mut diagram);
    diagram
}

fn append_graded_bars(
    diagram: &mut Vec<ProofBar>,
    complex: &GradedComplex,
    reduced: &[Vec<SparseColumn>],
    vertex_count: usize,
    dimension: usize,
) {
    let births = if dimension == 0 {
        vec![true; vertex_count]
    } else {
        reduced[dimension - 1]
            .iter()
            .map(|column| column.0.is_empty())
            .collect()
    };
    let deaths = reduced[dimension]
        .iter()
        .enumerate()
        .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
        .collect::<BTreeMap<_, _>>();
    for (birth_position, is_birth) in births.into_iter().enumerate() {
        if is_birth {
            append_graded_bar(diagram, complex, dimension, birth_position, &deaths);
        }
    }
}

fn append_graded_bar(
    diagram: &mut Vec<ProofBar>,
    complex: &GradedComplex,
    dimension: usize,
    birth_position: usize,
    deaths: &BTreeMap<usize, usize>,
) {
    let birth = complex.simplices[dimension][birth_position].value;
    let death = deaths
        .get(&birth_position)
        .map_or(f64::INFINITY, |&position| {
            complex.simplices[dimension + 1][position].value
        });
    if death > birth {
        diagram.push(ProofBar {
            dimension,
            birth,
            death,
        });
    }
}

pub(super) fn check_graded_diagram(diagram: &[ProofBar], max_dim: usize) -> Result<(), ProofError> {
    for bar in diagram {
        if bar.dimension > max_dim
            || !bar.birth.is_finite()
            || bar.birth < 0.0
            || bar.death.is_nan()
            || bar.death < 0.0
            || bar.death <= bar.birth
        {
            return Err(ProofError::new("graded diagram contains an invalid bar"));
        }
    }
    let mut canonical = diagram.to_vec();
    canonicalize_diagram(&mut canonical);
    if !diagrams_equal(&canonical, diagram) {
        return Err(ProofError::new("graded diagram bars are not canonical"));
    }
    Ok(())
}
