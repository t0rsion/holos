//! Exact selection over a declared portfolio of collapse schedules.
//!
//! A portfolio runs each listed schedule, verifies every removal, counts the
//! surviving flag simplices, and selects the lexicographic minimum. The claim
//! is exact over the declared candidates. It is not a claim that the selected
//! trace is globally optimal over all valid collapse sequences.

use std::cmp::Ordering;

use sha2::{Digest, Sha256};

use super::verify::{verify_sparse, verify_sparse_artifact};
use super::wire::{CollapseArtifact, DecodeLimits};
use super::{
    AdaptiveCollapseParams, CollapseCertificate, CollapseObjective, CollapsedRips, collapse_sparse,
    collapse_sparse_adaptive, collapse_sparse_rounds_parallel,
};
use crate::{Error, Result, SparseDistanceMatrix};

/// One schedule in an exact collapse portfolio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapsePortfolioCandidate {
    /// Serial version 1 collapse.
    Serial,
    /// Snapshot-round version 2 collapse with a worker budget.
    Rounds {
        /// Worker count for the schedule.
        threads: usize,
    },
    /// Score-ordered version 3 collapse.
    Adaptive {
        /// Downstream clique objective.
        objective: CollapseObjective,
        /// Optional removability-test limit.
        work_limit: Option<u64>,
    },
}

/// Exact objective used to compare collapsed graphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapsePortfolioObjective {
    /// Minimize the number of surviving edges.
    Edges,
    /// Minimize explicit reduction columns in the highest dimension first.
    ///
    /// Persistence through homology dimension `q` uses simplices through
    /// dimension `q + 1`. The score counts every surviving flag simplex in
    /// dimensions 1 through `q + 1`, then compares those counts from high to
    /// low dimension.
    ReductionColumns {
        /// Highest homology dimension served by the reduced graph.
        max_homology_dimension: usize,
    },
}

/// Resource limits for exact collapse portfolio selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollapsePortfolioLimits {
    /// Largest accepted candidate count.
    pub max_candidates: usize,
    /// Largest homology dimension accepted by the score counter.
    pub max_homology_dimension: usize,
    /// Largest nonvertex clique count visited for one candidate.
    pub max_cliques_per_candidate: u64,
}

impl Default for CollapsePortfolioLimits {
    fn default() -> Self {
        Self {
            max_candidates: 16,
            max_homology_dimension: 8,
            max_cliques_per_candidate: 100_000_000,
        }
    }
}

/// Exact surviving-simplex score for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollapsePortfolioScore {
    simplex_counts: Vec<u64>,
}

impl CollapsePortfolioScore {
    /// Counts in simplex dimensions 1, 2, and so on.
    pub fn simplex_counts(&self) -> &[u64] {
        &self.simplex_counts
    }

    fn compare(&self, other: &Self) -> Ordering {
        self.simplex_counts
            .iter()
            .rev()
            .cmp(other.simplex_counts.iter().rev())
    }
}

/// One verified candidate and its exact score.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioEntry {
    candidate: CollapsePortfolioCandidate,
    score: CollapsePortfolioScore,
    result: CollapsedRips,
}

impl CollapsePortfolioEntry {
    /// Schedule that produced this entry.
    pub fn candidate(&self) -> CollapsePortfolioCandidate {
        self.candidate
    }

    /// Exact score recomputed from the reduced graph.
    pub fn score(&self) -> &CollapsePortfolioScore {
        &self.score
    }

    /// Verified collapsed graph and removal certificate.
    pub fn result(&self) -> &CollapsedRips {
        &self.result
    }
}

/// Exact result over one declared collapse portfolio.
#[derive(Debug, Clone)]
pub struct CollapsePortfolio {
    objective: CollapsePortfolioObjective,
    entries: Vec<CollapsePortfolioEntry>,
    selected: usize,
}

impl CollapsePortfolio {
    /// Objective used to compare candidates.
    pub fn objective(&self) -> CollapsePortfolioObjective {
        self.objective
    }

    /// Verified candidates in caller order.
    pub fn entries(&self) -> &[CollapsePortfolioEntry] {
        &self.entries
    }

    /// Index of the exact lexicographic minimum.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Selected candidate.
    pub fn selected(&self) -> &CollapsePortfolioEntry {
        &self.entries[self.selected]
    }

    /// Recheck every certificate, score, and the final selection.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        threshold: Option<f64>,
        limits: CollapsePortfolioLimits,
    ) -> Result<()> {
        validate_request(
            &self
                .entries
                .iter()
                .map(|entry| entry.candidate)
                .collect::<Vec<_>>(),
            self.objective,
            limits,
        )?;
        if self.selected >= self.entries.len() {
            return Err(portfolio_error(
                "selected candidate is outside the portfolio",
            ));
        }
        for entry in &self.entries {
            verify_entry(input, threshold, self.objective, entry, limits)?;
        }
        let selected = select_entry(&self.entries);
        if selected != self.selected {
            return Err(portfolio_error(
                "selected candidate is not the exact minimum",
            ));
        }
        Ok(())
    }
}

/// Run and verify an exact portfolio of sparse collapse schedules.
///
/// Ties keep the first candidate in caller order. Candidate order is part of
/// the declared finite optimization problem.
pub fn collapse_sparse_portfolio(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    candidates: &[CollapsePortfolioCandidate],
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolio> {
    validate_request(candidates, objective, limits)?;
    let mut entries = Vec::with_capacity(candidates.len());
    for &candidate in candidates {
        let result = run_candidate(input, threshold, candidate)?;
        verify_sparse(input, threshold, &result)
            .map_err(|error| portfolio_error(format!("candidate certificate failed: {error}")))?;
        let score = score_graph(&result.matrix, objective, limits)?;
        entries.push(CollapsePortfolioEntry {
            candidate,
            score,
            result,
        });
    }
    let selected = select_entry(&entries);
    Ok(CollapsePortfolio {
        objective,
        entries,
        selected,
    })
}

fn validate_request(
    candidates: &[CollapsePortfolioCandidate],
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    if candidates.is_empty() || candidates.len() > limits.max_candidates {
        return Err(portfolio_error(
            "candidate count is zero or exceeds the portfolio limit",
        ));
    }
    validate_objective(objective, limits)?;
    for (index, candidate) in candidates.iter().enumerate() {
        validate_candidate(*candidate)?;
        if candidates[..index].contains(candidate) {
            return Err(portfolio_error("portfolio contains a duplicate candidate"));
        }
    }
    Ok(())
}

fn validate_objective(
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    let CollapsePortfolioObjective::ReductionColumns {
        max_homology_dimension,
    } = objective
    else {
        return Ok(());
    };
    if max_homology_dimension > limits.max_homology_dimension {
        return Err(portfolio_error(
            "portfolio homology dimension exceeds its limit",
        ));
    }
    Ok(())
}

fn validate_candidate(candidate: CollapsePortfolioCandidate) -> Result<()> {
    if let CollapsePortfolioCandidate::Rounds { threads: 0 } = candidate {
        return Err(portfolio_error(
            "rounds candidate needs at least one worker",
        ));
    }
    Ok(())
}

fn run_candidate(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    candidate: CollapsePortfolioCandidate,
) -> Result<CollapsedRips> {
    match candidate {
        CollapsePortfolioCandidate::Serial => collapse_sparse(input, threshold),
        CollapsePortfolioCandidate::Rounds { threads } => {
            collapse_sparse_rounds_parallel(input, threshold, threads)
        }
        CollapsePortfolioCandidate::Adaptive {
            objective,
            work_limit,
        } => {
            let mut params = AdaptiveCollapseParams::new(objective);
            params.work_limit = work_limit;
            collapse_sparse_adaptive(input, threshold, params)
        }
    }
}

fn verify_entry(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    objective: CollapsePortfolioObjective,
    entry: &CollapsePortfolioEntry,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    validate_certificate_profile(entry.candidate, &entry.result)?;
    verify_sparse(input, threshold, &entry.result)
        .map_err(|error| portfolio_error(format!("candidate certificate failed: {error}")))?;
    let expected = score_graph(&entry.result.matrix, objective, limits)?;
    if expected != entry.score {
        return Err(portfolio_error("candidate score does not match its graph"));
    }
    Ok(())
}

fn validate_certificate_profile(
    candidate: CollapsePortfolioCandidate,
    result: &CollapsedRips,
) -> Result<()> {
    validate_profile(candidate, &result.certificate)
}

fn validate_profile(
    candidate: CollapsePortfolioCandidate,
    certificate: &CollapseCertificate,
) -> Result<()> {
    let matches = match candidate {
        CollapsePortfolioCandidate::Serial => {
            certificate.algorithm_version() == 1 && certificate.objective().is_none()
        }
        CollapsePortfolioCandidate::Rounds { .. } => {
            certificate.algorithm_version() == 2 && certificate.objective().is_none()
        }
        CollapsePortfolioCandidate::Adaptive {
            objective,
            work_limit,
        } => {
            certificate.algorithm_version() == 3
                && certificate.objective() == Some(objective)
                && certificate.work_limit() == work_limit
        }
    };
    if !matches {
        return Err(portfolio_error(
            "candidate metadata does not match its certificate",
        ));
    }
    Ok(())
}

fn score_graph(
    graph: &SparseDistanceMatrix,
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolioScore> {
    match objective {
        CollapsePortfolioObjective::Edges => Ok(CollapsePortfolioScore {
            simplex_counts: vec![graph.num_edges() as u64],
        }),
        CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension,
        } => count_flag_simplices(graph, max_homology_dimension + 1, limits),
    }
}

fn count_flag_simplices(
    graph: &SparseDistanceMatrix,
    max_simplex_dimension: usize,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolioScore> {
    let adjacency = adjacency(graph);
    let vertices = (0..graph.len()).collect::<Vec<_>>();
    let mut counter = CliqueCounter {
        adjacency: &adjacency,
        counts: vec![0; max_simplex_dimension],
        maximum_size: max_simplex_dimension + 1,
        visited: 0,
        limit: limits.max_cliques_per_candidate,
    };
    counter.extend(0, &vertices)?;
    Ok(CollapsePortfolioScore {
        simplex_counts: counter.counts,
    })
}

fn adjacency(graph: &SparseDistanceMatrix) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); graph.len()];
    for (u, v, _) in graph.edges() {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    adjacency
}

struct CliqueCounter<'a> {
    adjacency: &'a [Vec<usize>],
    counts: Vec<u64>,
    maximum_size: usize,
    visited: u64,
    limit: u64,
}

impl CliqueCounter<'_> {
    fn extend(&mut self, prefix_size: usize, candidates: &[usize]) -> Result<()> {
        for (position, &vertex) in candidates.iter().enumerate() {
            let size = prefix_size + 1;
            if size >= 2 {
                self.count(size)?;
            }
            if size < self.maximum_size {
                let next = intersect(&candidates[position + 1..], &self.adjacency[vertex]);
                self.extend(size, &next)?;
            }
        }
        Ok(())
    }

    fn count(&mut self, clique_size: usize) -> Result<()> {
        self.visited = self
            .visited
            .checked_add(1)
            .ok_or_else(|| portfolio_error("portfolio clique count overflows"))?;
        if self.visited > self.limit {
            return Err(portfolio_error(
                "portfolio clique count exceeds its candidate limit",
            ));
        }
        let count = &mut self.counts[clique_size - 2];
        *count = count
            .checked_add(1)
            .ok_or_else(|| portfolio_error("portfolio simplex count overflows"))?;
        Ok(())
    }
}

fn intersect(left: &[usize], right: &[usize]) -> Vec<usize> {
    let mut output = Vec::new();
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            Ordering::Less => left_index += 1,
            Ordering::Greater => right_index += 1,
            Ordering::Equal => {
                output.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    output
}

fn select_entry(entries: &[CollapsePortfolioEntry]) -> usize {
    let mut selected = 0;
    for index in 1..entries.len() {
        if entries[index].score.compare(&entries[selected].score) == Ordering::Less {
            selected = index;
        }
    }
    selected
}

fn portfolio_error(message: impl Into<String>) -> Error {
    Error::InvalidInput(format!("collapse portfolio: {}", message.into()))
}

const PORTFOLIO_MAGIC: &[u8; 8] = b"HOLOSPOR";
const PORTFOLIO_VERSION: u16 = 1;

/// Decoder limits for a portable collapse portfolio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollapsePortfolioDecodeLimits {
    /// Largest accepted outer envelope.
    pub max_bytes: usize,
    /// Largest accepted candidate count.
    pub max_candidates: usize,
    /// Limits for every nested collapse artifact.
    pub collapse: DecodeLimits,
}

impl Default for CollapsePortfolioDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_candidates: 16,
            collapse: DecodeLimits::default(),
        }
    }
}

/// One candidate carried by a portable collapse portfolio.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioArtifactEntry {
    candidate: CollapsePortfolioCandidate,
    score: CollapsePortfolioScore,
    artifact: CollapseArtifact,
}

impl CollapsePortfolioArtifactEntry {
    /// Declared schedule.
    pub fn candidate(&self) -> CollapsePortfolioCandidate {
        self.candidate
    }

    /// Declared exact score.
    pub fn score(&self) -> &CollapsePortfolioScore {
        &self.score
    }

    /// Nested collapse proof.
    pub fn artifact(&self) -> &CollapseArtifact {
        &self.artifact
    }
}

/// Portable proof of exact selection over a finite collapse portfolio.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioArtifact {
    objective: CollapsePortfolioObjective,
    entries: Vec<CollapsePortfolioArtifactEntry>,
    selected: usize,
    digest: [u8; 32],
}

impl CollapsePortfolioArtifact {
    /// Build a portable artifact from a computed portfolio.
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

    /// Exact score objective.
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

    /// Recheck every collapse proof, exact score, and the selected minimum.
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

    fn compute_digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.encode_payload()?).into())
    }
}

fn decode_portfolio_digest(bytes: &[u8], maximum: usize) -> Result<(&[u8], [u8; 32])> {
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

fn decode_portfolio_payload(
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

fn encode_portfolio_objective(
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

fn encode_portfolio_entry(
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

fn put_portfolio_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_portfolio_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_portfolio_usize(output: &mut Vec<u8>, value: usize, label: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 1.0),
                (0, 2, 1.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
                (3, 4, 1.0),
                (3, 5, 1.0),
                (4, 5, 1.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn clique_counter_counts_edges_triangles_and_tetrahedra() {
        let score = count_flag_simplices(&graph(), 3, CollapsePortfolioLimits::default()).unwrap();
        assert_eq!(score.simplex_counts(), &[9, 5, 1]);
    }

    #[test]
    fn portfolio_selects_the_exact_declared_minimum() {
        let input = graph();
        let candidates = [
            CollapsePortfolioCandidate::Serial,
            CollapsePortfolioCandidate::Rounds { threads: 2 },
            CollapsePortfolioCandidate::Adaptive {
                objective: CollapseObjective::H2,
                work_limit: None,
            },
        ];
        let portfolio = collapse_sparse_portfolio(
            &input,
            None,
            &candidates,
            CollapsePortfolioObjective::ReductionColumns {
                max_homology_dimension: 2,
            },
            CollapsePortfolioLimits::default(),
        )
        .unwrap();
        portfolio
            .verify(&input, None, CollapsePortfolioLimits::default())
            .unwrap();
        let selected = portfolio.selected().score();
        assert!(
            portfolio
                .entries()
                .iter()
                .all(|entry| selected.compare(entry.score()) != Ordering::Greater)
        );
    }

    #[test]
    fn portfolio_artifact_round_trips_and_rechecks_every_candidate() {
        let input = graph();
        let candidates = [
            CollapsePortfolioCandidate::Serial,
            CollapsePortfolioCandidate::Rounds { threads: 2 },
            CollapsePortfolioCandidate::Adaptive {
                objective: CollapseObjective::H1,
                work_limit: Some(100),
            },
        ];
        let objective = CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension: 2,
        };
        let portfolio = collapse_sparse_portfolio(
            &input,
            None,
            &candidates,
            objective,
            CollapsePortfolioLimits::default(),
        )
        .unwrap();
        let artifact = CollapsePortfolioArtifact::from_portfolio(
            &portfolio,
            CollapsePortfolioLimits::default(),
        )
        .unwrap();
        let bytes = artifact
            .encode(
                CollapsePortfolioLimits::default(),
                CollapsePortfolioDecodeLimits::default(),
            )
            .unwrap();
        let decoded = CollapsePortfolioArtifact::decode(
            &bytes,
            CollapsePortfolioLimits::default(),
            CollapsePortfolioDecodeLimits::default(),
        )
        .unwrap();
        decoded
            .verify_sparse(&input, None, CollapsePortfolioLimits::default())
            .unwrap();
        assert_eq!(decoded.objective(), objective);
        assert_eq!(decoded.selected_index(), portfolio.selected_index());
        assert_eq!(decoded.entries().len(), candidates.len());
    }

    #[test]
    fn portfolio_artifact_rejects_mutation_and_a_false_selection() {
        let input = graph();
        let portfolio = collapse_sparse_portfolio(
            &input,
            None,
            &[
                CollapsePortfolioCandidate::Serial,
                CollapsePortfolioCandidate::Rounds { threads: 2 },
            ],
            CollapsePortfolioObjective::Edges,
            CollapsePortfolioLimits::default(),
        )
        .unwrap();
        let mut artifact = CollapsePortfolioArtifact::from_portfolio(
            &portfolio,
            CollapsePortfolioLimits::default(),
        )
        .unwrap();
        let mut bytes = artifact
            .encode(
                CollapsePortfolioLimits::default(),
                CollapsePortfolioDecodeLimits::default(),
            )
            .unwrap();
        bytes[12] ^= 1;
        assert!(
            CollapsePortfolioArtifact::decode(
                &bytes,
                CollapsePortfolioLimits::default(),
                CollapsePortfolioDecodeLimits::default(),
            )
            .is_err()
        );

        artifact.selected = (artifact.selected + 1) % artifact.entries.len();
        artifact.digest = artifact.compute_digest().unwrap();
        assert!(
            artifact
                .verify_sparse(&input, None, CollapsePortfolioLimits::default())
                .is_err()
        );
    }

    #[test]
    fn portfolio_rejects_duplicates_and_zero_workers() {
        let limits = CollapsePortfolioLimits::default();
        assert!(
            validate_request(
                &[
                    CollapsePortfolioCandidate::Serial,
                    CollapsePortfolioCandidate::Serial,
                ],
                CollapsePortfolioObjective::Edges,
                limits,
            )
            .is_err()
        );
        assert!(
            validate_request(
                &[CollapsePortfolioCandidate::Rounds { threads: 0 }],
                CollapsePortfolioObjective::Edges,
                limits,
            )
            .is_err()
        );
    }

    #[test]
    fn clique_limit_stops_before_unbounded_materialization() {
        let limits = CollapsePortfolioLimits {
            max_cliques_per_candidate: 2,
            ..CollapsePortfolioLimits::default()
        };
        assert!(count_flag_simplices(&graph(), 3, limits).is_err());
    }
}
