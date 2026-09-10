use sha2::{Digest, Sha256};

use super::super::CollapseObjective;
use super::super::wire::{CollapseArtifact, DecodeLimits};
use super::model::{
    CollapsePortfolioArtifact, CollapsePortfolioArtifactEntry, CollapsePortfolioCandidate,
    CollapsePortfolioDecodeLimits, CollapsePortfolioLimits, CollapsePortfolioObjective,
    CollapsePortfolioScore,
};
use super::selection::portfolio_error;
use crate::Result;

pub(super) const PORTFOLIO_MAGIC: &[u8; 8] = b"HOLOSPOR";
pub(super) const PORTFOLIO_VERSION: u16 = 1;

pub(super) fn decode_portfolio_digest(bytes: &[u8], maximum: usize) -> Result<(&[u8], [u8; 32])> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(portfolio_error(
            "portfolio is truncated or exceeds its byte limit",
        ));
    }
    let payload_length = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_length]).into();
    let digest: [u8; 32] = bytes[payload_length..]
        .try_into()
        .expect("32-byte portfolio digest");
    if digest != expected {
        return Err(portfolio_error("portfolio digest does not match its bytes"));
    }
    Ok((&bytes[..payload_length], digest))
}

pub(super) fn decode_portfolio_payload(
    payload: &[u8],
    digest: [u8; 32],
    portfolio_limits: CollapsePortfolioLimits,
    decode_limits: CollapsePortfolioDecodeLimits,
) -> Result<CollapsePortfolioArtifact> {
    let mut reader = PortfolioReader::new(payload);
    decode_portfolio_prefix(&mut reader)?;
    let objective = decode_portfolio_objective(&mut reader)?;
    let candidate_limit = decode_limits
        .max_candidates
        .min(portfolio_limits.max_candidates);
    let count = reader.bounded_usize("candidate count", candidate_limit)?;
    let selected = reader.usize()?;
    let entries = decode_portfolio_entries(&mut reader, count, objective, decode_limits.collapse)?;
    reader.finish()?;
    Ok(CollapsePortfolioArtifact {
        objective,
        entries,
        selected,
        digest,
    })
}

fn decode_portfolio_entries(
    reader: &mut PortfolioReader<'_>,
    count: usize,
    objective: CollapsePortfolioObjective,
    limits: DecodeLimits,
) -> Result<Vec<CollapsePortfolioArtifactEntry>> {
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push(decode_portfolio_entry(reader, objective, limits)?);
    }
    Ok(entries)
}

pub(super) fn encode_portfolio_objective(
    output: &mut Vec<u8>,
    objective: CollapsePortfolioObjective,
) -> Result<()> {
    match objective {
        CollapsePortfolioObjective::Edges => output.push(1),
        CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension,
        } => {
            output.push(2);
            put_portfolio_usize(
                output,
                max_homology_dimension,
                "objective homology dimension",
            )?;
        }
    }
    Ok(())
}

fn decode_portfolio_objective(
    reader: &mut PortfolioReader<'_>,
) -> Result<CollapsePortfolioObjective> {
    match reader.u8()? {
        1 => Ok(CollapsePortfolioObjective::Edges),
        2 => Ok(CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension: reader.usize()?,
        }),
        _ => Err(portfolio_error("portfolio objective tag is invalid")),
    }
}

pub(super) fn encode_portfolio_entry(
    output: &mut Vec<u8>,
    entry: &CollapsePortfolioArtifactEntry,
) -> Result<()> {
    encode_portfolio_candidate(output, entry.candidate)?;
    put_portfolio_usize(output, entry.score.simplex_counts.len(), "score length")?;
    for &count in &entry.score.simplex_counts {
        put_portfolio_u64(output, count);
    }
    let artifact = entry
        .artifact
        .encode()
        .map_err(|error| portfolio_error(error.to_string()))?;
    put_portfolio_usize(output, artifact.len(), "candidate byte count")?;
    output.extend_from_slice(&artifact);
    Ok(())
}

fn decode_portfolio_entry(
    reader: &mut PortfolioReader<'_>,
    objective: CollapsePortfolioObjective,
    limits: DecodeLimits,
) -> Result<CollapsePortfolioArtifactEntry> {
    let candidate = decode_portfolio_candidate(reader)?;
    let expected_score_length = score_length(objective);
    let score_length = reader.bounded_usize("score length", expected_score_length)?;
    if score_length != expected_score_length {
        return Err(portfolio_error("candidate score has the wrong dimension"));
    }
    let mut simplex_counts = Vec::with_capacity(score_length);
    for _ in 0..score_length {
        simplex_counts.push(reader.u64()?);
    }
    let byte_count = reader.bounded_usize("candidate byte count", limits.max_bytes)?;
    let bytes = reader.take(byte_count)?;
    let artifact = CollapseArtifact::decode(bytes, limits)
        .map_err(|error| portfolio_error(error.to_string()))?;
    Ok(CollapsePortfolioArtifactEntry {
        candidate,
        score: CollapsePortfolioScore { simplex_counts },
        artifact,
    })
}

fn score_length(objective: CollapsePortfolioObjective) -> usize {
    match objective {
        CollapsePortfolioObjective::Edges => 1,
        CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension,
        } => max_homology_dimension.saturating_add(1),
    }
}

fn encode_portfolio_candidate(
    output: &mut Vec<u8>,
    candidate: CollapsePortfolioCandidate,
) -> Result<()> {
    match candidate {
        CollapsePortfolioCandidate::Serial => output.push(1),
        CollapsePortfolioCandidate::Rounds { threads } => {
            output.push(2);
            put_portfolio_usize(output, threads, "rounds worker count")?;
        }
        CollapsePortfolioCandidate::Adaptive {
            objective,
            work_limit,
        } => {
            output.push(match objective {
                CollapseObjective::H1 => 3,
                CollapseObjective::H2 => 4,
            });
            encode_optional_work_limit(output, work_limit);
        }
    }
    Ok(())
}

fn decode_portfolio_candidate(
    reader: &mut PortfolioReader<'_>,
) -> Result<CollapsePortfolioCandidate> {
    match reader.u8()? {
        1 => Ok(CollapsePortfolioCandidate::Serial),
        2 => Ok(CollapsePortfolioCandidate::Rounds {
            threads: reader.usize()?,
        }),
        3 => Ok(CollapsePortfolioCandidate::Adaptive {
            objective: CollapseObjective::H1,
            work_limit: decode_optional_work_limit(reader)?,
        }),
        4 => Ok(CollapsePortfolioCandidate::Adaptive {
            objective: CollapseObjective::H2,
            work_limit: decode_optional_work_limit(reader)?,
        }),
        _ => Err(portfolio_error("portfolio candidate tag is invalid")),
    }
}

fn encode_optional_work_limit(output: &mut Vec<u8>, limit: Option<u64>) {
    match limit {
        None => output.push(0),
        Some(limit) => {
            output.push(1);
            put_portfolio_u64(output, limit);
        }
    }
}

fn decode_optional_work_limit(reader: &mut PortfolioReader<'_>) -> Result<Option<u64>> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.u64()?)),
        _ => Err(portfolio_error("portfolio work-limit tag is invalid")),
    }
}

fn decode_portfolio_prefix(reader: &mut PortfolioReader<'_>) -> Result<()> {
    if reader.take(8)? != PORTFOLIO_MAGIC || reader.u16()? != PORTFOLIO_VERSION {
        return Err(portfolio_error("portfolio envelope version is unsupported"));
    }
    Ok(())
}

pub(super) fn put_portfolio_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_portfolio_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_portfolio_usize(output: &mut Vec<u8>, value: usize, label: &str) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| portfolio_error(format!("{label} does not fit the wire integer")))?;
    put_portfolio_u64(output, value);
    Ok(())
}

struct PortfolioReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PortfolioReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| portfolio_error("portfolio read position overflows"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| portfolio_error("portfolio is truncated"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte portfolio slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .expect("eight-byte portfolio slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| portfolio_error("portfolio integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > limit {
            return Err(portfolio_error(format!(
                "{label} {value} exceeds its limit {limit}"
            )));
        }
        Ok(value)
    }

    fn finish(&self) -> Result<()> {
        if self.position != self.bytes.len() {
            return Err(portfolio_error("portfolio has trailing payload bytes"));
        }
        Ok(())
    }
}
