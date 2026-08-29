use std::time::Instant;

use holos_tda::SparseDistanceMatrix;
use holos_tda::collapse::{
    CollapseObjective, CollapsePortfolioArtifact, CollapsePortfolioCandidate,
    CollapsePortfolioDecodeLimits, CollapsePortfolioLimits, CollapsePortfolioObjective,
    collapse_sparse_portfolio,
};

use crate::{Measurement, median};

pub(crate) fn measure(repetitions: usize) -> Result<Measurement, String> {
    let input = specimen()?;
    let candidates = [
        CollapsePortfolioCandidate::Serial,
        CollapsePortfolioCandidate::Rounds { threads: 4 },
        CollapsePortfolioCandidate::Adaptive {
            objective: CollapseObjective::H1,
            work_limit: None,
        },
    ];
    let objective = CollapsePortfolioObjective::ReductionColumns {
        max_homology_dimension: 1,
    };
    let limits = CollapsePortfolioLimits::default();
    let decode_limits = CollapsePortfolioDecodeLimits::default();
    let mut producer_times = Vec::with_capacity(repetitions);
    let mut checker_times = Vec::with_capacity(repetitions);
    let mut final_bytes = Vec::new();
    let mut selected = 0;
    for _ in 0..repetitions {
        let started = Instant::now();
        let portfolio = collapse_sparse_portfolio(&input, None, &candidates, objective, limits)
            .map_err(|error| error.to_string())?;
        let artifact = CollapsePortfolioArtifact::from_portfolio(&portfolio, limits)
            .map_err(|error| error.to_string())?;
        let bytes = artifact
            .encode(limits, decode_limits)
            .map_err(|error| error.to_string())?;
        producer_times.push(started.elapsed());
        let started = Instant::now();
        let decoded = CollapsePortfolioArtifact::decode(&bytes, limits, decode_limits)
            .map_err(|error| error.to_string())?;
        decoded
            .verify_sparse(&input, None, limits)
            .map_err(|error| error.to_string())?;
        checker_times.push(started.elapsed());
        selected = decoded.selected_index();
        final_bytes = bytes;
    }
    Ok(Measurement {
        family: "portfolio",
        case: "overlapping-cliques",
        producer: median(producer_times),
        checker: median(checker_times),
        artifact_bytes: final_bytes.len(),
        work: format!("candidates=3 selected={selected} checker=linked"),
    })
}

fn specimen() -> Result<SparseDistanceMatrix, String> {
    let mut edges = Vec::new();
    for range in [0..6, 4..10, 8..14] {
        for u in range.clone() {
            for v in u + 1..range.end {
                edges.push((u, v, 1.0 + (u + v) as f64 / 100.0));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(14, &edges).map_err(|error| error.to_string())
}
