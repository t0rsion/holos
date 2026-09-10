use std::cmp::Ordering;

use sha2::{Digest, Sha256};

use super::super::verify::verify_sparse_artifact;
use super::super::wire::CollapseArtifact;
use super::model::{
    CollapsePortfolio, CollapsePortfolioArtifact, CollapsePortfolioArtifactEntry,
    CollapsePortfolioDecodeLimits, CollapsePortfolioLimits, CollapsePortfolioObjective,
};
use super::selection::{portfolio_error, score_graph, validate_profile, validate_request};
use super::wire::{
    PORTFOLIO_MAGIC, PORTFOLIO_VERSION, decode_portfolio_digest, decode_portfolio_payload,
    encode_portfolio_entry, encode_portfolio_objective, put_portfolio_u16, put_portfolio_usize,
};
use crate::{Result, SparseDistanceMatrix};

impl CollapsePortfolioArtifact {
    /// Build an artifact from a computed portfolio.
    pub fn from_portfolio(
        portfolio: &CollapsePortfolio,
        limits: CollapsePortfolioLimits,
    ) -> Result<Self> {
        let mut entries = Vec::with_capacity(portfolio.entries.len());
        for entry in &portfolio.entries {
            let artifact = CollapseArtifact::from_result(&entry.result)
                .map_err(|error| portfolio_error(error.to_string()))?;
            entries.push(CollapsePortfolioArtifactEntry {
                candidate: entry.candidate,
                score: entry.score.clone(),
                artifact,
            });
        }
        let mut artifact = Self {
            objective: portfolio.objective,
            entries,
            selected: portfolio.selected,
            digest: [0; 32],
        };
        artifact.validate_structure(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Score objective.
    pub fn objective(&self) -> CollapsePortfolioObjective {
        self.objective
    }

    /// Candidates in declared order.
    pub fn entries(&self) -> &[CollapsePortfolioArtifactEntry] {
        &self.entries
    }

    /// Index of the selected candidate.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Selected nested collapse proof.
    pub fn selected(&self) -> &CollapsePortfolioArtifactEntry {
        &self.entries[self.selected]
    }

    /// Content digest of the outer envelope.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Recheck every collapse proof, score, and the selected minimum.
    pub fn verify_sparse(
        &self,
        input: &SparseDistanceMatrix,
        threshold: Option<f64>,
        limits: CollapsePortfolioLimits,
    ) -> Result<()> {
        self.validate_structure(limits)?;
        if self.compute_digest()? != self.digest {
            return Err(portfolio_error(
                "portfolio digest does not match its payload",
            ));
        }
        for entry in &self.entries {
            verify_sparse_artifact(input, threshold, &entry.artifact)
                .map_err(|error| portfolio_error(format!("candidate artifact failed: {error}")))?;
        }
        Ok(())
    }

    /// Encode canonical `HOLOSPOR` version 1 bytes.
    pub fn encode(
        &self,
        portfolio_limits: CollapsePortfolioLimits,
        decode_limits: CollapsePortfolioDecodeLimits,
    ) -> Result<Vec<u8>> {
        self.validate_structure(portfolio_limits)?;
        if self.compute_digest()? != self.digest {
            return Err(portfolio_error(
                "portfolio digest does not match its payload",
            ));
        }
        let mut bytes = self.encode_payload()?;
        bytes.extend_from_slice(&self.digest);
        if bytes.len() > decode_limits.max_bytes {
            return Err(portfolio_error("portfolio exceeds its byte limit"));
        }
        Ok(bytes)
    }

    /// Decode and structurally verify canonical `HOLOSPOR` version 1 bytes.
    pub fn decode(
        bytes: &[u8],
        portfolio_limits: CollapsePortfolioLimits,
        decode_limits: CollapsePortfolioDecodeLimits,
    ) -> Result<Self> {
        let (payload, digest) = decode_portfolio_digest(bytes, decode_limits.max_bytes)?;
        let artifact = decode_portfolio_payload(payload, digest, portfolio_limits, decode_limits)?;
        artifact.validate_structure(portfolio_limits)?;
        Ok(artifact)
    }

    fn validate_structure(&self, limits: CollapsePortfolioLimits) -> Result<()> {
        let candidates = self
            .entries
            .iter()
            .map(|entry| entry.candidate)
            .collect::<Vec<_>>();
        validate_request(&candidates, self.objective, limits)?;
        if self.selected >= self.entries.len() {
            return Err(portfolio_error(
                "selected candidate is outside the portfolio",
            ));
        }
        let first = &self.entries[0].artifact;
        for entry in &self.entries {
            validate_profile(entry.candidate, entry.artifact.certificate())?;
            validate_common_input(first, &entry.artifact)?;
            let score = score_graph(entry.artifact.matrix(), self.objective, limits)?;
            if score != entry.score {
                return Err(portfolio_error("candidate score does not match its graph"));
            }
        }
        if select_artifact_entry(&self.entries) != self.selected {
            return Err(portfolio_error(
                "selected candidate is not the exact minimum",
            ));
        }
        Ok(())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(PORTFOLIO_MAGIC);
        put_portfolio_u16(&mut output, PORTFOLIO_VERSION);
        encode_portfolio_objective(&mut output, self.objective)?;
        put_portfolio_usize(&mut output, self.entries.len(), "candidate count")?;
        put_portfolio_usize(&mut output, self.selected, "selected candidate")?;
        for entry in &self.entries {
            encode_portfolio_entry(&mut output, entry)?;
        }
        Ok(output)
    }

    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.encode_payload()?).into())
    }
}
fn validate_common_input(reference: &CollapseArtifact, candidate: &CollapseArtifact) -> Result<()> {
    if reference.input_digest() != candidate.input_digest()
        || !same_optional_float(
            reference.certificate().requested_threshold(),
            candidate.certificate().requested_threshold(),
        )
    {
        return Err(portfolio_error(
            "candidate artifacts do not bind one thresholded input",
        ));
    }
    Ok(())
}

fn same_optional_float(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
        _ => false,
    }
}

fn select_artifact_entry(entries: &[CollapsePortfolioArtifactEntry]) -> usize {
    let mut selected = 0;
    for index in 1..entries.len() {
        if entries[index].score.compare(&entries[selected].score) == Ordering::Less {
            selected = index;
        }
    }
    selected
}
