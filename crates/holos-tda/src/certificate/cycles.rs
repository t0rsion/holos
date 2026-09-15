//! Geometric H1 cycle witnesses derived from a checked reduction.

use std::collections::{BTreeMap, HashMap};

use crate::{Cocycle, CriticalPair, SparseDistanceMatrix};

use super::model::{
    CertificateError, CertificateLimits, CertificateResult, ChangeColumn, ReductionCertificate,
};
use super::reduction::{FilteredComplex, SparseColumn};
use super::verify::{CheckedReductions, checked_threshold, inverse_mod};

/// A geometric cycle and, for a finite pair, a chain that bounds it.
///
/// The terms use increasing vertex orientation. The cycle and chain terms are
/// ordered by the filtered simplex order used by the reduction. This object
/// records endpoint evidence. It does not assign a canonical class to one
/// critical pair.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CycleWitness {
    pub(crate) pair: CriticalPair,
    pub(crate) cycle: Vec<(usize, usize, u32)>,
    pub(crate) bounding_chain: Vec<([usize; 3], u32)>,
}

pub(super) struct CheckedWitnessParts<'a> {
    certificate: &'a ReductionCertificate,
    complex: &'a FilteredComplex,
    checked: &'a CheckedReductions,
}

impl<'a> CheckedWitnessParts<'a> {
    pub(super) fn new(
        certificate: &'a ReductionCertificate,
        complex: &'a FilteredComplex,
        checked: &'a CheckedReductions,
    ) -> Self {
        Self {
            certificate,
            complex,
            checked,
        }
    }
}

/// Extract a witness from an existing checked reduction.
#[cfg(test)]
fn cycle_witness_from_certificate(
    certificate: &ReductionCertificate,
    input: &SparseDistanceMatrix,
    threshold: f64,
    modulus: u32,
    critical_pairs: &[CriticalPair],
    selected: &Cocycle,
    limits: CertificateLimits,
) -> CertificateResult<CycleWitness> {
    let threshold = validate_witness_request(
        certificate,
        input,
        threshold,
        modulus,
        critical_pairs,
        selected,
        limits,
    )?;
    let (complex, checked) = certificate.verify_parts(input, limits)?;
    let parts = CheckedWitnessParts::new(certificate, &complex, &checked);
    extract_from_checked(
        input,
        threshold,
        modulus,
        critical_pairs,
        selected,
        limits,
        &parts,
    )
}

/// Extract a witness from reduction parts checked during construction.
pub(super) fn cycle_witness_from_checked(
    parts: CheckedWitnessParts<'_>,
    input: &SparseDistanceMatrix,
    threshold: f64,
    modulus: u32,
    critical_pairs: &[CriticalPair],
    selected: &Cocycle,
    limits: CertificateLimits,
) -> CertificateResult<CycleWitness> {
    let threshold = validate_witness_request(
        parts.certificate,
        input,
        threshold,
        modulus,
        critical_pairs,
        selected,
        limits,
    )?;
    extract_from_checked(
        input,
        threshold,
        modulus,
        critical_pairs,
        selected,
        limits,
        &parts,
    )
}

fn validate_witness_request(
    certificate: &ReductionCertificate,
    input: &SparseDistanceMatrix,
    threshold: f64,
    modulus: u32,
    critical_pairs: &[CriticalPair],
    selected: &Cocycle,
    limits: CertificateLimits,
) -> CertificateResult<f64> {
    let threshold = checked_threshold(Some(threshold))?;
    if checked_threshold(certificate.threshold())?.to_bits() != threshold.to_bits() {
        return Err(CertificateError::new(
            "cycle witness threshold differs from the reduction certificate",
        ));
    }
    if certificate.modulus() != modulus {
        return Err(CertificateError::new(
            "cycle witness modulus differs from the reduction certificate",
        ));
    }
    if critical_pairs.len() > limits.max_bars {
        return Err(CertificateError::new(format!(
            "{} critical pairs exceed the limit {}",
            critical_pairs.len(),
            limits.max_bars
        )));
    }
    check_selected(input, threshold, modulus, selected, limits)?;
    Ok(threshold)
}

fn extract_from_checked(
    input: &SparseDistanceMatrix,
    threshold: f64,
    modulus: u32,
    critical_pairs: &[CriticalPair],
    selected: &Cocycle,
    limits: CertificateLimits,
    parts: &CheckedWitnessParts<'_>,
) -> CertificateResult<CycleWitness> {
    let search = WitnessSearch {
        complex: parts.complex,
        checked: parts.checked,
        certificate: parts.certificate,
        edge_positions: parts
            .complex
            .edges
            .iter()
            .enumerate()
            .map(|(index, edge)| (edge.vertices, index))
            .collect(),
        triangle_positions: parts
            .complex
            .triangles
            .iter()
            .enumerate()
            .map(|(index, triangle)| (triangle.vertices, index))
            .collect(),
        selected_coefficients: selected_coefficients(selected),
        selected_scale_bits: selected.scale.to_bits(),
        terminal_scale: input
            .edges()
            .map(|(_, _, value)| value)
            .fold(0.0, f64::max)
            .min(threshold),
        modulus,
        limits,
    };
    let mut saw_matching_scale = false;

    for pair in critical_pairs {
        let Some(candidate) = search.candidate_for_pair(pair)? else {
            continue;
        };
        saw_matching_scale = true;
        if let Some(witness) = search.normalize_candidate(pair, &candidate)? {
            return Ok(witness);
        }
    }
    if !saw_matching_scale {
        return Err(CertificateError::new(
            "selected cocycle scale does not match any critical pair",
        ));
    }
    Err(CertificateError::new(
        "selected cocycle has zero pairing with every critical pair cycle",
    ))
}

struct WitnessSearch<'a> {
    complex: &'a FilteredComplex,
    checked: &'a CheckedReductions,
    certificate: &'a ReductionCertificate,
    edge_positions: HashMap<[usize; 2], usize>,
    triangle_positions: HashMap<[usize; 3], usize>,
    selected_coefficients: BTreeMap<(usize, usize), u64>,
    selected_scale_bits: u64,
    terminal_scale: f64,
    modulus: u32,
    limits: CertificateLimits,
}

impl<'a> WitnessSearch<'a> {
    fn candidate_for_pair(&self, pair: &CriticalPair) -> CertificateResult<Option<Candidate>> {
        let (birth_index, death_index) = pair_positions(
            pair,
            self.complex,
            &self.edge_positions,
            &self.triangle_positions,
        )?;
        let candidate_scale = representative_scale(death_index, self.complex, self.terminal_scale)?;
        if self.selected_scale_bits != candidate_scale.to_bits() {
            return Ok(None);
        }
        Ok(Some(self.build_candidate(
            pair,
            birth_index,
            death_index,
        )?))
    }

    fn build_candidate(
        &self,
        pair: &CriticalPair,
        birth_index: usize,
        death_index: Option<usize>,
    ) -> CertificateResult<Candidate> {
        match death_index {
            Some(death_index) => self.finite_candidate(pair, birth_index, death_index),
            None => self.essential_candidate(pair, birth_index),
        }
    }

    fn normalize_candidate(
        &self,
        pair: &CriticalPair,
        candidate: &Candidate,
    ) -> CertificateResult<Option<CycleWitness>> {
        let pairing_value = pairing(&self.selected_coefficients, &candidate.cycle, self.modulus);
        if pairing_value == 0 {
            return Ok(None);
        }
        let factor = inverse_mod(pairing_value, self.modulus as u64);
        let cycle = scale_cycle(&candidate.cycle, factor, self.modulus);
        let bounding_chain = scale_chain(&candidate.bounding_chain, factor, self.modulus);
        check_scaled_boundary(&cycle, &bounding_chain, self.modulus)?;
        if pairing(&self.selected_coefficients, &cycle, self.modulus) != 1 {
            return Err(CertificateError::new(
                "scaled cycle witness does not pair to one",
            ));
        }
        Ok(Some(CycleWitness {
            pair: pair.clone(),
            cycle,
            bounding_chain,
        }))
    }

    fn finite_candidate(
        &self,
        pair: &CriticalPair,
        birth_index: usize,
        death_index: usize,
    ) -> CertificateResult<Candidate> {
        let reduced = finite_reduced_column(birth_index, death_index, self.checked)?;
        let transform = self
            .certificate
            .triangle_columns()
            .get(death_index)
            .ok_or_else(|| CertificateError::new("death triangle has no change column"))?;
        // The checked factorization gives boundary(transform) = reduced. The
        // reduced triangle column is therefore the cycle bounded by this chain.
        let cycle = edge_terms(
            reduced,
            self.complex,
            birth_index,
            "finite birth cycle",
            self.modulus,
        )?;
        let bounding_chain = triangle_terms(
            transform,
            self.complex,
            death_index,
            "finite death chain",
            self.modulus,
        )?;
        check_unit_death_transform(transform, death_index)?;
        check_limits(cycle.len(), bounding_chain.len(), self.limits)?;
        check_chain_boundary(&cycle, &bounding_chain, self.modulus, pair)?;
        Ok(Candidate {
            cycle,
            bounding_chain,
        })
    }

    fn essential_candidate(
        &self,
        pair: &CriticalPair,
        birth_index: usize,
    ) -> CertificateResult<Candidate> {
        let reduced = self
            .checked
            .reduced_edges
            .get(birth_index)
            .ok_or_else(|| CertificateError::new("birth edge is outside the reduction"))?;
        if !reduced.0.is_empty() {
            return Err(CertificateError::new(
                "essential critical pair has a nonzero reduced edge column",
            ));
        }
        let transform = self
            .certificate
            .edge_columns()
            .get(birth_index)
            .ok_or_else(|| CertificateError::new("birth edge has no change column"))?;
        if target_coefficient(&transform.terms, birth_index) != Some(1) {
            return Err(CertificateError::new(
                "essential birth cycle is not unit triangular at its birth edge",
            ));
        }
        // A zero reduced edge column makes its change column a cycle directly;
        // no second reduction is needed for an essential candidate.
        let cycle = edge_terms(
            &SparseColumn(
                transform
                    .terms
                    .iter()
                    .map(|term| (term.index, term.coefficient as u64))
                    .collect(),
            ),
            self.complex,
            birth_index,
            "essential birth cycle",
            self.modulus,
        )?;
        check_limits(cycle.len(), 0, self.limits)?;
        check_zero_boundary(&cycle, self.modulus, pair)?;
        Ok(Candidate {
            cycle,
            bounding_chain: Vec::new(),
        })
    }
}

struct Candidate {
    cycle: Vec<(usize, usize, u32)>,
    bounding_chain: Vec<([usize; 3], u32)>,
}

fn representative_scale(
    death_index: Option<usize>,
    complex: &FilteredComplex,
    terminal_scale: f64,
) -> CertificateResult<f64> {
    death_index.map_or(Ok(terminal_scale), |index| {
        previous_float_checked(complex.triangles[index].value)
    })
}

fn finite_reduced_column(
    birth_index: usize,
    death_index: usize,
    checked: &CheckedReductions,
) -> CertificateResult<&SparseColumn> {
    let birth_reduced = checked
        .reduced_edges
        .get(birth_index)
        .ok_or_else(|| CertificateError::new("birth edge is outside the reduction"))?;
    if !birth_reduced.0.is_empty() {
        return Err(CertificateError::new(
            "finite critical pair has a nonzero reduced birth edge column",
        ));
    }
    let reduced = checked
        .reduced_triangles
        .get(death_index)
        .ok_or_else(|| CertificateError::new("death triangle is outside the reduction"))?;
    if reduced.pivot().map(|(index, _)| index) != Some(birth_index) {
        return Err(CertificateError::new(
            "critical pair does not match the reduced death column",
        ));
    }
    Ok(reduced)
}

fn check_unit_death_transform(
    transform: &ChangeColumn,
    death_index: usize,
) -> CertificateResult<()> {
    if target_coefficient(&transform.terms, death_index) != Some(1) {
        return Err(CertificateError::new(
            "finite death chain is not unit triangular at its death simplex",
        ));
    }
    Ok(())
}

fn pair_positions(
    pair: &CriticalPair,
    complex: &FilteredComplex,
    edge_positions: &HashMap<[usize; 2], usize>,
    triangle_positions: &HashMap<[usize; 3], usize>,
) -> CertificateResult<(usize, Option<usize>)> {
    let birth: [usize; 2] = pair
        .birth
        .vertices
        .as_slice()
        .try_into()
        .map_err(|_| CertificateError::new("critical birth simplex is not an edge"))?;
    let [u, v] = birth;
    if u >= v {
        return Err(CertificateError::new(
            "critical birth edge is not in increasing vertex order",
        ));
    }
    let birth_index = *edge_positions
        .get(&birth)
        .ok_or_else(|| CertificateError::new("critical birth edge is not in the filtration"))?;
    if complex.edges[birth_index].value.to_bits() != pair.birth.value.to_bits() {
        return Err(CertificateError::new(
            "critical birth edge value differs from the filtration",
        ));
    }
    let death_index =
        pair.death
            .as_ref()
            .map(|death| {
                let vertices: [usize; 3] = death.vertices.as_slice().try_into().map_err(|_| {
                    CertificateError::new("critical death simplex is not a triangle")
                })?;
                let [u, v, w] = vertices;
                if !(u < v && v < w) {
                    return Err(CertificateError::new(
                        "critical death triangle is not in increasing vertex order",
                    ));
                }
                let index = *triangle_positions.get(&vertices).ok_or_else(|| {
                    CertificateError::new("critical death triangle is not in the filtration")
                })?;
                if complex.triangles[index].value.to_bits() != death.value.to_bits() {
                    return Err(CertificateError::new(
                        "critical death triangle value differs from the filtration",
                    ));
                }
                if !pair.birth.value.total_cmp(&death.value).is_lt() {
                    return Err(CertificateError::new(
                        "finite critical pair does not have a strictly later death",
                    ));
                }
                Ok(index)
            })
            .transpose()?;
    Ok((birth_index, death_index))
}

fn check_selected(
    input: &SparseDistanceMatrix,
    threshold: f64,
    modulus: u32,
    selected: &Cocycle,
    limits: CertificateLimits,
) -> CertificateResult<()> {
    if selected.modulus != modulus {
        return Err(CertificateError::new(
            "selected cocycle modulus differs from the witness field",
        ));
    }
    if selected.scale > threshold {
        return Err(CertificateError::new(
            "selected cocycle scale exceeds the witness threshold",
        ));
    }
    if selected.terms.len() > limits.max_edges {
        return Err(CertificateError::new(format!(
            "{} selected cocycle terms exceed the edge limit {}",
            selected.terms.len(),
            limits.max_edges
        )));
    }
    if selected.terms.len() > limits.max_terms {
        return Err(CertificateError::new(format!(
            "{} selected cocycle terms exceed the term limit {}",
            selected.terms.len(),
            limits.max_terms
        )));
    }
    crate::classes::validate_h1_cocycle(input, selected)
        .map_err(|error| CertificateError::new(error.to_string()))
}

fn selected_coefficients(selected: &Cocycle) -> BTreeMap<(usize, usize), u64> {
    selected
        .terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient as u64))
        .collect()
}

fn edge_terms(
    column: &SparseColumn,
    complex: &FilteredComplex,
    maximum_index: usize,
    label: &str,
    modulus: u32,
) -> CertificateResult<Vec<(usize, usize, u32)>> {
    if column.0.is_empty() {
        return Err(CertificateError::new(format!("{label} is empty")));
    }
    let mut previous = None;
    let mut terms = Vec::with_capacity(column.0.len());
    for (&index, &coefficient) in &column.0 {
        if index > maximum_index || previous.is_some_and(|value| value >= index) {
            return Err(CertificateError::new(format!(
                "{label} does not follow the filtered simplex order"
            )));
        }
        if coefficient == 0 || coefficient >= modulus as u64 {
            return Err(CertificateError::new(format!(
                "{label} has an invalid coefficient"
            )));
        }
        let edge = complex
            .edges
            .get(index)
            .ok_or_else(|| CertificateError::new(format!("{label} names an absent edge")))?;
        terms.push((edge.vertices[0], edge.vertices[1], coefficient as u32));
        previous = Some(index);
    }
    Ok(terms)
}

fn triangle_terms(
    column: &ChangeColumn,
    complex: &FilteredComplex,
    maximum_index: usize,
    label: &str,
    modulus: u32,
) -> CertificateResult<Vec<([usize; 3], u32)>> {
    if column.terms.is_empty() {
        return Err(CertificateError::new(format!("{label} is empty")));
    }
    let mut previous = None;
    let mut terms = Vec::with_capacity(column.terms.len());
    for term in &column.terms {
        if term.index > maximum_index || previous.is_some_and(|value| value >= term.index) {
            return Err(CertificateError::new(format!(
                "{label} does not follow the filtered simplex order"
            )));
        }
        if term.coefficient == 0 || term.coefficient >= modulus {
            return Err(CertificateError::new(format!(
                "{label} has an invalid coefficient"
            )));
        }
        let triangle = complex
            .triangles
            .get(term.index)
            .ok_or_else(|| CertificateError::new(format!("{label} names an absent triangle")))?;
        terms.push((triangle.vertices, term.coefficient));
        previous = Some(term.index);
    }
    Ok(terms)
}

fn target_coefficient(terms: &[super::model::CertificateTerm], target: usize) -> Option<u32> {
    terms
        .iter()
        .find(|term| term.index == target)
        .map(|term| term.coefficient)
}

fn check_limits(
    cycle_len: usize,
    chain_len: usize,
    limits: CertificateLimits,
) -> CertificateResult<()> {
    if cycle_len > limits.max_edges {
        return Err(CertificateError::new(format!(
            "{cycle_len} cycle terms exceed the edge limit {}",
            limits.max_edges
        )));
    }
    if chain_len > limits.max_triangles {
        return Err(CertificateError::new(format!(
            "{chain_len} death-chain terms exceed the triangle limit {}",
            limits.max_triangles
        )));
    }
    let total = cycle_len
        .checked_add(chain_len)
        .ok_or_else(|| CertificateError::new("cycle witness term count overflows usize"))?;
    if total > limits.max_terms {
        return Err(CertificateError::new(format!(
            "{total} cycle witness terms exceed the term limit {}",
            limits.max_terms
        )));
    }
    Ok(())
}

fn pairing(
    selected: &BTreeMap<(usize, usize), u64>,
    cycle: &[(usize, usize, u32)],
    modulus: u32,
) -> u64 {
    let modulus = modulus as u64;
    cycle.iter().fold(0, |sum, &(u, v, coefficient)| {
        (sum + selected.get(&(u, v)).copied().unwrap_or(0) * coefficient as u64) % modulus
    })
}

fn scale_cycle(
    cycle: &[(usize, usize, u32)],
    factor: u64,
    modulus: u32,
) -> Vec<(usize, usize, u32)> {
    cycle
        .iter()
        .map(|&(u, v, coefficient)| {
            (
                u,
                v,
                ((coefficient as u64 * factor) % modulus as u64) as u32,
            )
        })
        .collect()
}

fn scale_chain(chain: &[([usize; 3], u32)], factor: u64, modulus: u32) -> Vec<([usize; 3], u32)> {
    chain
        .iter()
        .map(|&(vertices, coefficient)| {
            (
                vertices,
                ((coefficient as u64 * factor) % modulus as u64) as u32,
            )
        })
        .collect()
}

fn check_chain_boundary(
    cycle: &[(usize, usize, u32)],
    chain: &[([usize; 3], u32)],
    modulus: u32,
    pair: &CriticalPair,
) -> CertificateResult<()> {
    let actual = chain_boundary(chain, modulus);
    let expected = edge_map(cycle);
    if actual != expected {
        return Err(CertificateError::new(format!(
            "death chain does not bound the cycle for birth {:?}",
            pair.birth.vertices
        )));
    }
    Ok(())
}

fn check_scaled_boundary(
    cycle: &[(usize, usize, u32)],
    chain: &[([usize; 3], u32)],
    modulus: u32,
) -> CertificateResult<()> {
    if chain.is_empty() {
        if cycle.is_empty() {
            return Err(CertificateError::new("scaled essential cycle is empty"));
        }
        return Ok(());
    }
    if chain_boundary(chain, modulus) != edge_map(cycle) {
        return Err(CertificateError::new(
            "scaled death chain does not bound the scaled cycle",
        ));
    }
    Ok(())
}

fn check_zero_boundary(
    cycle: &[(usize, usize, u32)],
    modulus: u32,
    pair: &CriticalPair,
) -> CertificateResult<()> {
    let mut boundary = BTreeMap::new();
    for &(u, v, coefficient) in cycle {
        add_coefficient(&mut boundary, u, coefficient as u64, modulus);
        add_coefficient(
            &mut boundary,
            v,
            modulus as u64 - coefficient as u64,
            modulus,
        );
    }
    if !boundary.is_empty() {
        return Err(CertificateError::new(format!(
            "essential cycle does not close for birth {:?}",
            pair.birth.vertices
        )));
    }
    Ok(())
}

fn edge_map(cycle: &[(usize, usize, u32)]) -> BTreeMap<(usize, usize), u64> {
    cycle
        .iter()
        .map(|&(u, v, coefficient)| ((u, v), coefficient as u64))
        .collect()
}

fn chain_boundary(chain: &[([usize; 3], u32)], modulus: u32) -> BTreeMap<(usize, usize), u64> {
    let mut boundary = BTreeMap::new();
    for &([u, v, w], coefficient) in chain {
        let coefficient = coefficient as u64;
        add_edge_coefficient(&mut boundary, (v, w), coefficient, modulus);
        add_edge_coefficient(&mut boundary, (u, w), modulus as u64 - coefficient, modulus);
        add_edge_coefficient(&mut boundary, (u, v), coefficient, modulus);
    }
    boundary
}

fn add_edge_coefficient(
    map: &mut BTreeMap<(usize, usize), u64>,
    edge: (usize, usize),
    coefficient: u64,
    modulus: u32,
) {
    add_coefficient_at(map, edge, coefficient % modulus as u64, modulus);
}

fn add_coefficient_at(
    map: &mut BTreeMap<(usize, usize), u64>,
    key: (usize, usize),
    coefficient: u64,
    modulus: u32,
) {
    let next = (map.get(&key).copied().unwrap_or(0) + coefficient) % modulus as u64;
    if next == 0 {
        map.remove(&key);
    } else {
        map.insert(key, next);
    }
}

fn add_coefficient(map: &mut BTreeMap<usize, u64>, key: usize, coefficient: u64, modulus: u32) {
    let next = (map.get(&key).copied().unwrap_or(0) + coefficient) % modulus as u64;
    if next == 0 {
        map.remove(&key);
    } else {
        map.insert(key, next);
    }
}

fn previous_float_checked(value: f64) -> CertificateResult<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(CertificateError::new(
            "finite death value cannot have a predecessor scale",
        ));
    }
    Ok(f64::from_bits(value.to_bits() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RipsParams, rips_persistence_with_classes_sparse};

    fn finite_square() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn finite_witness_has_pairing_one_and_matching_boundary() {
        let input = finite_square();
        let params = RipsParams::new(1).with_threshold(2.0).with_modulus(5);
        let certificate =
            ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
        let explained = rips_persistence_with_classes_sparse(&input, &params).unwrap();
        let space = explained.spaces.first().unwrap();
        let selected = &space.basis[0].cocycle;
        let witness = cycle_witness_from_certificate(
            &certificate,
            &input,
            2.0,
            5,
            &space.critical_pairs,
            selected,
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(!witness.cycle.is_empty());
        assert!(!witness.bounding_chain.is_empty());
        assert_eq!(
            pairing(&selected_coefficients(selected), &witness.cycle, 5),
            1
        );
        assert_eq!(
            chain_boundary(&witness.bounding_chain, 5),
            edge_map(&witness.cycle)
        );
    }

    #[test]
    fn essential_witness_uses_a_zero_edge_column() {
        let input = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let params = RipsParams::new(1).with_threshold(1.0).with_modulus(5);
        let certificate =
            ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
        let explained = rips_persistence_with_classes_sparse(&input, &params).unwrap();
        let space = explained.spaces.first().unwrap();
        let witness = cycle_witness_from_certificate(
            &certificate,
            &input,
            1.0,
            5,
            &space.critical_pairs,
            &space.basis[0].cocycle,
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(witness.pair.death.is_none());
        assert!(!witness.cycle.is_empty());
        assert!(witness.bounding_chain.is_empty());
    }
}
