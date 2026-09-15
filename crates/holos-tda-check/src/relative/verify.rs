use std::collections::BTreeSet;

use super::super::{ProofError, ProofLimits, diagrams_equal};
use super::digest::certificate_digest;
use super::model::{Cell, VerifiedCertificate};
use super::reduction::{check_chain, check_protected, check_reduction};
use super::replay::replay;

pub(super) fn verify_certificate(
    certificate: &VerifiedCertificate,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    check_chain(&certificate.input, certificate.max_dim, certificate.modulus)?;
    check_chain(&certificate.core, certificate.max_dim, certificate.modulus)?;
    let protected = certificate
        .protected_vertices
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    verify_protected_scope(&certificate.input, &protected)?;
    let replayed = replay(
        certificate.input.clone(),
        &certificate.steps,
        &protected,
        certificate.modulus,
    )?;
    verify_replay(certificate, replayed, &protected)?;
    verify_certificate_reduction(certificate, limits)?;
    verify_certificate_digest(certificate)
}

fn verify_protected_scope(
    input: &[Vec<Cell>],
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    let input_vertices = input
        .first()
        .ok_or_else(|| ProofError::new("relative interface has no input vertex dimension"))?;
    let labels = input_vertices
        .iter()
        .map(|cell| cell.vertices.first().copied())
        .collect::<Option<BTreeSet<_>>>()
        .ok_or_else(|| ProofError::new("relative input vertex cell has no label"))?;
    if protected.iter().any(|vertex| !labels.contains(vertex)) {
        return Err(ProofError::new(
            "relative protected vertex is outside the input complex",
        ));
    }
    Ok(())
}

pub(super) fn verify_replay(
    certificate: &VerifiedCertificate,
    replayed: Vec<Vec<Cell>>,
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    if replayed != certificate.core {
        return Err(ProofError::new(
            "relative cancellation trace does not produce the declared core",
        ));
    }
    check_protected(&certificate.input, &certificate.core, protected)
}

pub(super) fn verify_certificate_reduction(
    certificate: &VerifiedCertificate,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    let checked = check_reduction(
        &certificate.core,
        certificate.modulus,
        &certificate.columns,
        limits,
    )?;
    if !diagrams_equal(&checked, &certificate.diagram) {
        return Err(ProofError::new(
            "relative-interface diagram differs from the checked reduction",
        ));
    }
    Ok(())
}

pub(super) fn verify_certificate_digest(
    certificate: &VerifiedCertificate,
) -> Result<(), ProofError> {
    let computed = certificate_digest(
        certificate.max_dim,
        certificate.modulus,
        &certificate.protected_vertices,
        &certificate.core,
        &certificate.columns,
        &certificate.diagram,
    );
    if computed != certificate.digest {
        Err(ProofError::new(
            "relative-interface digest differs from checked content",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(label: usize) -> Cell {
        Cell {
            vertices: vec![label],
            value: 0.0,
            boundary: Vec::new(),
        }
    }

    #[test]
    fn protected_scope_rejects_vertex_absent_from_input() {
        let input = vec![vec![vertex(3)], Vec::new()];
        let protected = BTreeSet::from([4]);
        let error = verify_protected_scope(&input, &protected).unwrap_err();
        assert_eq!(
            error.message(),
            "relative protected vertex is outside the input complex"
        );
    }

    #[test]
    fn protected_scope_accepts_input_vertex_label() {
        let input = vec![vec![vertex(3)], Vec::new()];
        let protected = BTreeSet::from([3]);
        verify_protected_scope(&input, &protected).unwrap();
    }
}
