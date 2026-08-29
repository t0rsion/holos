use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::collapse::{
    CollapseObjective, CollapsePortfolioArtifact, CollapsePortfolioCandidate,
    CollapsePortfolioDecodeLimits, CollapsePortfolioLimits, CollapsePortfolioObjective,
    collapse_sparse_portfolio,
};
use crate::io;

use super::{version_string, write_via_temporary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CandidateArg {
    Serial,
    Rounds,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ScoreArg {
    Edges,
    Columns,
}

#[derive(Parser)]
#[command(
    name = "holos collapse-portfolio",
    version = version_string(),
    about = "Select and certify the best declared collapse schedule"
)]
pub(super) struct CollapsePortfolioCli {
    /// Sparse `i j d` input.
    input: PathBuf,

    /// Output `HOLOSPOR` artifact.
    output: PathBuf,

    /// Schedules in tie-breaking order.
    #[arg(
        long = "candidate",
        value_enum,
        value_delimiter = ',',
        default_value = "serial,rounds,adaptive"
    )]
    candidates: Vec<CandidateArg>,

    /// Filtration threshold. The default includes every listed edge.
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Objective used to compare surviving graphs.
    #[arg(long, value_enum, default_value_t = ScoreArg::Columns)]
    score: ScoreArg,

    /// Highest homology dimension served by a column score.
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Worker count for the rounds schedule and input parser.
    #[arg(long, value_name = "N", default_value_t = 4)]
    threads: usize,

    /// Downstream objective for the adaptive schedule.
    #[arg(long, value_enum, default_value_t = super::ObjectiveArg::H1)]
    adaptive_objective: super::ObjectiveArg,

    /// Removability-test limit for the adaptive schedule.
    #[arg(long, value_name = "N")]
    adaptive_work_limit: Option<u64>,

    /// Largest nonvertex clique count visited for one schedule.
    #[arg(long, value_name = "N", default_value_t = 100_000_000)]
    max_cliques: u64,

    /// Largest accepted output artifact.
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

pub(super) fn run_collapse_portfolio(cli: CollapsePortfolioCli) -> crate::Result<()> {
    let graph = io::read_sparse_matrix(&cli.input, cli.threads)?;
    let candidates = cli
        .candidates
        .iter()
        .map(|candidate| portfolio_candidate(*candidate, &cli))
        .collect::<Vec<_>>();
    let objective = match cli.score {
        ScoreArg::Edges => CollapsePortfolioObjective::Edges,
        ScoreArg::Columns => CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension: cli.dim,
        },
    };
    let limits = CollapsePortfolioLimits {
        max_candidates: candidates.len(),
        max_homology_dimension: cli.dim,
        max_cliques_per_candidate: cli.max_cliques,
    };
    let portfolio =
        collapse_sparse_portfolio(&graph, cli.threshold, &candidates, objective, limits)?;
    let artifact = CollapsePortfolioArtifact::from_portfolio(&portfolio, limits)?;
    let decode_limits = CollapsePortfolioDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_candidates: candidates.len(),
        ..CollapsePortfolioDecodeLimits::default()
    };
    let bytes = artifact.encode(limits, decode_limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    print_portfolio(&portfolio, bytes.len());
    Ok(())
}

fn portfolio_candidate(
    candidate: CandidateArg,
    cli: &CollapsePortfolioCli,
) -> CollapsePortfolioCandidate {
    match candidate {
        CandidateArg::Serial => CollapsePortfolioCandidate::Serial,
        CandidateArg::Rounds => CollapsePortfolioCandidate::Rounds {
            threads: cli.threads,
        },
        CandidateArg::Adaptive => CollapsePortfolioCandidate::Adaptive {
            objective: match cli.adaptive_objective {
                super::ObjectiveArg::H1 => CollapseObjective::H1,
                super::ObjectiveArg::H2 => CollapseObjective::H2,
            },
            work_limit: cli.adaptive_work_limit,
        },
    }
}

fn print_portfolio(portfolio: &crate::collapse::CollapsePortfolio, bytes: usize) {
    for (index, entry) in portfolio.entries().iter().enumerate() {
        println!(
            "candidate {index}: {}, simplex counts {:?}, {} surviving edges",
            candidate_name(entry.candidate()),
            entry.score().simplex_counts(),
            entry.result().matrix.num_edges(),
        );
    }
    println!(
        "selected candidate {} ({}) and wrote {bytes} bytes",
        portfolio.selected_index(),
        candidate_name(portfolio.selected().candidate()),
    );
}

fn candidate_name(candidate: CollapsePortfolioCandidate) -> &'static str {
    match candidate {
        CollapsePortfolioCandidate::Serial => "serial",
        CollapsePortfolioCandidate::Rounds { .. } => "rounds",
        CollapsePortfolioCandidate::Adaptive { .. } => "adaptive",
    }
}
