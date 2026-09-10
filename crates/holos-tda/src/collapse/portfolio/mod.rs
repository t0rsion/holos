//! Exact selection over a declared portfolio of collapse schedules.
//!
//! A portfolio runs each listed schedule, verifies every removal, counts the
//! surviving flag simplices, and selects the lexicographic minimum. The claim
//! is exact over the declared candidates. It does not rank traces outside
//! that list.

mod artifact;
mod model;
mod selection;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    CollapsePortfolio, CollapsePortfolioArtifact, CollapsePortfolioArtifactEntry,
    CollapsePortfolioCandidate, CollapsePortfolioDecodeLimits, CollapsePortfolioEntry,
    CollapsePortfolioLimits, CollapsePortfolioObjective, CollapsePortfolioScore,
};
pub use selection::collapse_sparse_portfolio;
