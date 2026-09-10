use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    ProofBar, ProofColumn, ProofError, ProofLimits, SparseColumn, canonicalize_diagram,
    check_column,
};
use super::cells::add_term_count;
use super::digest::cell_order;
use super::model::{Cell, Term};

pub(super) fn check_chain(
    cells: &[Vec<Cell>],
    max_dim: usize,
    modulus: u32,
) -> Result<(), ProofError> {
    if cells.len() != max_dim + 2 {
        return Err(ProofError::new(
            "relative interface has the wrong dimension count",
        ));
    }
    let maps: Vec<BTreeMap<_, _>> = cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect()
        })
        .collect();
    for (dimension, values) in cells.iter().enumerate() {
        check_chain_dimension(dimension, values, &maps, modulus)?;
    }
    for dimension in 2..cells.len() {
        for cell in &cells[dimension] {
            check_boundary_square(cell, &maps[dimension - 1], modulus)?;
        }
    }
    Ok(())
}

pub(super) fn check_chain_dimension(
    dimension: usize,
    cells: &[Cell],
    maps: &[BTreeMap<Vec<usize>, &Cell>],
    modulus: u32,
) -> Result<(), ProofError> {
    let mut previous = None;
    for cell in cells {
        check_cell(dimension, cell, previous)?;
        check_cell_boundary(dimension, cell, maps, modulus)?;
        previous = Some(cell);
    }
    if maps[dimension].len() != cells.len() {
        Err(ProofError::new("relative interface repeats a cell"))
    } else {
        Ok(())
    }
}

pub(super) fn check_cell(
    dimension: usize,
    cell: &Cell,
    previous: Option<&Cell>,
) -> Result<(), ProofError> {
    if cell.vertices.len() != dimension + 1
        || !cell.value.is_finite()
        || cell.value < 0.0
        || previous.is_some_and(|prior| cell_order(prior, cell).is_gt())
    {
        Err(ProofError::new(
            "relative-interface cell order or value is invalid",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn check_cell_boundary(
    dimension: usize,
    cell: &Cell,
    maps: &[BTreeMap<Vec<usize>, &Cell>],
    modulus: u32,
) -> Result<(), ProofError> {
    let mut prior = None;
    for term in &cell.boundary {
        let face = maps
            .get(dimension.wrapping_sub(1))
            .and_then(|rows| rows.get(&term.cell))
            .ok_or_else(|| ProofError::new("relative boundary cell is absent"))?;
        if face.value > cell.value
            || term.coefficient == 0
            || term.coefficient >= modulus
            || prior.is_some_and(|key: &[usize]| key >= term.cell.as_slice())
        {
            return Err(ProofError::new(
                "relative boundary is not canonical and filtered",
            ));
        }
        prior = Some(term.cell.as_slice());
    }
    Ok(())
}

pub(super) fn check_boundary_square(
    cell: &Cell,
    lower_cells: &BTreeMap<Vec<usize>, &Cell>,
    modulus: u32,
) -> Result<(), ProofError> {
    let mut square = BTreeMap::<Vec<usize>, u64>::new();
    for term in &cell.boundary {
        for lower in &lower_cells[&term.cell].boundary {
            add_square_term(&mut square, term, lower, modulus);
        }
    }
    if square.is_empty() {
        Ok(())
    } else {
        Err(ProofError::new(
            "relative-interface boundary does not square to zero",
        ))
    }
}

pub(super) fn add_square_term(
    square: &mut BTreeMap<Vec<usize>, u64>,
    term: &Term,
    lower: &Term,
    modulus: u32,
) {
    let value = (square.get(&lower.cell).copied().unwrap_or(0)
        + u64::from(term.coefficient) * u64::from(lower.coefficient))
        % u64::from(modulus);
    if value == 0 {
        square.remove(&lower.cell);
    } else {
        square.insert(lower.cell.clone(), value);
    }
}

pub(super) fn check_protected(
    input: &[Vec<Cell>],
    core: &[Vec<Cell>],
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    for (before, after) in input.iter().zip(core) {
        let retained: BTreeMap<_, _> = after.iter().map(|cell| (&cell.vertices, cell)).collect();
        for cell in before {
            if is_protected(&cell.vertices, protected)
                && retained.get(&cell.vertices).copied() != Some(cell)
            {
                return Err(ProofError::new(
                    "relative interface does not fix its protected subcomplex",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn is_protected(cell: &[usize], protected: &BTreeSet<usize>) -> bool {
    !protected.is_empty() && cell.iter().all(|vertex| protected.contains(vertex))
}

pub(super) fn check_reduction(
    cells: &[Vec<Cell>],
    modulus: u32,
    columns: &[Vec<ProofColumn>],
    limits: ProofLimits,
) -> Result<Vec<ProofBar>, ProofError> {
    let boundaries = boundary_matrices(cells)?;
    if columns.len() != boundaries.len() {
        return Err(ProofError::new(
            "relative reduction has the wrong dimension count",
        ));
    }
    let reduced = reduce_boundaries(&boundaries, columns, modulus, limits)?;
    Ok(reduction_diagram(cells, columns, &reduced))
}

pub(super) fn reduce_boundaries(
    boundaries: &[Vec<SparseColumn>],
    columns: &[Vec<ProofColumn>],
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let mut total_terms = 0usize;
    let mut reduced = Vec::with_capacity(columns.len());
    for (matrix, transforms) in boundaries.iter().zip(columns) {
        reduced.push(reduce_dimension(
            matrix,
            transforms,
            modulus,
            &mut total_terms,
            limits,
        )?);
    }
    Ok(reduced)
}

pub(super) fn reduce_dimension(
    matrix: &[SparseColumn],
    transforms: &[ProofColumn],
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<SparseColumn>, ProofError> {
    if matrix.len() != transforms.len() {
        return Err(ProofError::new(
            "relative reduction has the wrong column count",
        ));
    }
    let mut values = Vec::with_capacity(matrix.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in transforms.iter().enumerate() {
        let result = reduce_column(matrix, transform, target, modulus, total_terms, limits)?;
        if result
            .pivot()
            .is_some_and(|(pivot, _)| !pivots.insert(pivot))
        {
            return Err(ProofError::new("relative reduced matrix repeats a pivot"));
        }
        values.push(result);
    }
    Ok(values)
}

pub(super) fn reduce_column(
    matrix: &[SparseColumn],
    transform: &ProofColumn,
    target: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<SparseColumn, ProofError> {
    check_column(target, &transform.terms, modulus)?;
    add_term_count(
        total_terms,
        transform.terms.len(),
        limits.max_terms,
        "relative change",
    )?;
    let mut result = SparseColumn::default();
    for term in &transform.terms {
        result.add_scaled(
            &matrix[term.index],
            u64::from(term.coefficient),
            u64::from(modulus),
        );
    }
    Ok(result)
}

pub(super) fn reduction_diagram(
    cells: &[Vec<Cell>],
    columns: &[Vec<ProofColumn>],
    reduced: &[Vec<SparseColumn>],
) -> Vec<ProofBar> {
    let mut diagram = Vec::new();
    for dimension in 0..columns.len() {
        append_dimension_bars(&mut diagram, cells, reduced, dimension);
    }
    canonicalize_diagram(&mut diagram);
    diagram
}

pub(super) fn append_dimension_bars(
    diagram: &mut Vec<ProofBar>,
    cells: &[Vec<Cell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) {
    let births = if dimension == 0 {
        vec![true; cells[0].len()]
    } else {
        reduced[dimension - 1]
            .iter()
            .map(|column| column.0.is_empty())
            .collect()
    };
    let deaths = reduced[dimension]
        .iter()
        .enumerate()
        .filter_map(|(column, value)| value.pivot().map(|(row, _)| (row, column)))
        .collect::<BTreeMap<_, _>>();
    for (position, is_birth) in births.into_iter().enumerate() {
        if is_birth {
            append_bar(diagram, cells, dimension, position, &deaths);
        }
    }
}

pub(super) fn append_bar(
    diagram: &mut Vec<ProofBar>,
    cells: &[Vec<Cell>],
    dimension: usize,
    position: usize,
    deaths: &BTreeMap<usize, usize>,
) {
    let birth = cells[dimension][position].value;
    let death = deaths
        .get(&position)
        .map_or(f64::INFINITY, |&column| cells[dimension + 1][column].value);
    if death > birth {
        diagram.push(ProofBar {
            dimension,
            birth,
            death,
        });
    }
}

pub(super) fn boundary_matrices(cells: &[Vec<Cell>]) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let rows: Vec<BTreeMap<_, _>> = cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, cell)| (cell.vertices.clone(), position))
                .collect()
        })
        .collect();
    (1..cells.len())
        .map(|dimension| {
            cells[dimension]
                .iter()
                .map(|cell| {
                    let mut column = SparseColumn::default();
                    for term in &cell.boundary {
                        let row = rows[dimension - 1].get(&term.cell).ok_or_else(|| {
                            ProofError::new("relative boundary references an absent cell")
                        })?;
                        column.insert(*row, term.coefficient as u64);
                    }
                    Ok(column)
                })
                .collect()
        })
        .collect()
}
