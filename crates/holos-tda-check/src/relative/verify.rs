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
