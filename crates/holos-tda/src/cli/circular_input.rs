//! Input parsing for the circular-coordinate command.

use std::path::Path;

use crate::IntegralCocycleTerm;

use super::input::read_bounded_artifact;

pub(crate) fn read_integral_lift(
    path: &Path,
    maximum_bytes: usize,
    vertex_count: usize,
) -> crate::Result<Vec<IntegralCocycleTerm>> {
    let bytes = read_bounded_artifact(path, maximum_bytes, "circular integral lift")?;
    let text = std::str::from_utf8(&bytes).map_err(|error| {
        crate::Error::InvalidInput(format!(
            "circular integral lift {} is not UTF-8: {error}",
            path.display()
        ))
    })?;
    let mut terms = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let term = parse_integral_lift_line(path, line_number, line, vertex_count)?;
        validate_integral_lift_order(path, line_number, terms.last(), term)?;
        terms.push(term);
    }
    if terms.is_empty() {
        return Err(crate::Error::InvalidInput(
            "circular integral lift has no terms".into(),
        ));
    }
    Ok(terms)
}

fn parse_integral_lift_line(
    path: &Path,
    line_number: usize,
    line: &str,
    vertex_count: usize,
) -> crate::Result<IntegralCocycleTerm> {
    let fields = line
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if fields.len() != 3 {
        return Err(crate::Error::InvalidInput(format!(
            "{}:{}: expected u v coefficient",
            path.display(),
            line_number
        )));
    }
    let u = fields[0].parse::<usize>().map_err(|error| {
        crate::Error::InvalidInput(format!(
            "{}:{}: invalid first endpoint: {error}",
            path.display(),
            line_number
        ))
    })?;
    let v = fields[1].parse::<usize>().map_err(|error| {
        crate::Error::InvalidInput(format!(
            "{}:{}: invalid second endpoint: {error}",
            path.display(),
            line_number
        ))
    })?;
    let coefficient = fields[2].parse::<i64>().map_err(|error| {
        crate::Error::InvalidInput(format!(
            "{}:{}: invalid integer coefficient: {error}",
            path.display(),
            line_number
        ))
    })?;
    if u >= v || v >= vertex_count {
        return Err(crate::Error::InvalidInput(format!(
            "{}:{}: integral lift edge must satisfy 0 <= u < v < {vertex_count}",
            path.display(),
            line_number
        )));
    }
    if coefficient == 0 {
        return Err(crate::Error::InvalidInput(format!(
            "{}:{}: integral lift coefficients must be nonzero",
            path.display(),
            line_number
        )));
    }
    if coefficient.unsigned_abs() > (1u64 << 31) {
        return Err(crate::Error::InvalidInput(format!(
            "{}:{}: integral lift coefficient exceeds 2^31",
            path.display(),
            line_number
        )));
    }
    Ok(IntegralCocycleTerm { u, v, coefficient })
}

fn validate_integral_lift_order(
    path: &Path,
    line_number: usize,
    previous: Option<&IntegralCocycleTerm>,
    term: IntegralCocycleTerm,
) -> crate::Result<()> {
    if previous.is_some_and(|previous| (previous.u, previous.v) >= (term.u, term.v)) {
        return Err(crate::Error::InvalidInput(format!(
            "{}:{}: integral lift terms are not in strict endpoint order",
            path.display(),
            line_number
        )));
    }
    Ok(())
}
