use std::collections::BTreeSet;

use crate::Diagram;
use crate::certificate::{CertificateError, CertificateLimits};

use super::cancellation::replay_cancellations;
use super::chain::check_chain_complex;
use super::digest::{certificate_digest, diagrams_equal};
use super::model::RelativeInterfaceCertificate;
use super::reduction::check_reduction;
use super::validation::{check_interface_parameters, check_protected_cells, enforce_cell_limits};

impl RelativeInterfaceCertificate {
    /// Replay every cancellation and check the retained reduction.
    pub fn verify(&self, limits: CertificateLimits) -> Result<Diagram, CertificateError> {
        check_interface_parameters(self.max_dim, self.modulus, limits)?;
        enforce_cell_limits(&self.input_cells, limits)?;
        enforce_cell_limits(&self.core_cells, limits)?;
        check_chain_complex(&self.input_cells, self.max_dim, self.modulus, limits)?;
        let protected: BTreeSet<_> = self.protected_vertices.iter().copied().collect();
        check_cancellation_replay(self, &protected)?;
        check_protected_cells(&self.input_cells, &self.core_cells, &protected)?;
        let (diagram, _) = check_reduction(&self.core_cells, self.modulus, &self.columns, limits)?;
        check_interface_result(self, &diagram)?;
        Ok(diagram)
    }
}

fn check_cancellation_replay(
    certificate: &RelativeInterfaceCertificate,
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    let replayed = replay_cancellations(
        certificate.input_cells.clone(),
        &certificate.cancellations,
        protected,
        certificate.modulus,
    )?;
    if replayed != certificate.core_cells {
        return Err(CertificateError::new(
            "relative cancellation trace does not produce the declared core",
        ));
    }
    Ok(())
}

fn check_interface_result(
    certificate: &RelativeInterfaceCertificate,
    diagram: &Diagram,
) -> Result<(), CertificateError> {
    if !diagrams_equal(diagram, &certificate.diagram) {
        return Err(CertificateError::new(
            "relative interface diagram differs from its checked reduction",
        ));
    }
    let digest = certificate_digest(
        certificate.max_dim,
        certificate.modulus,
        &certificate.protected_vertices,
        &certificate.core_cells,
        &certificate.columns,
        &certificate.diagram,
    );
    if digest != certificate.digest {
        return Err(CertificateError::new(
            "relative interface digest differs from its checked content",
        ));
    }
    Ok(())
}
