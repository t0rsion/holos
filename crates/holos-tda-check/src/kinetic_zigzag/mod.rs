mod decomposition;
mod model;
mod schedule;
mod wire;

use crate::cohomology::Space;
use crate::{ProofError, ProofLimits};

use self::decomposition::decompose;
use self::model::{KineticClaim, Map};
use self::schedule::{event_graphs, exact_schedule};
use self::wire::decode_claim;

/// Return true when bytes start with the kinetic zigzag magic.
pub fn is_kinetic_zigzag(bytes: &[u8]) -> bool {
    bytes.starts_with(wire::MAGIC)
}

/// Verify a `HOLOSZZ` artifact.
///
/// The checker reconstructs every exact event complex, canonical cohomology
/// basis, restriction map, generalized rank, and interval multiplicity.
pub fn verify_kinetic_zigzag(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedKineticZigzag, ProofError> {
    let claim = decode_claim(bytes, limits)?;
    let schedule = verify_schedule(&claim, limits)?;
    let graphs = event_graphs(
        &claim.trajectories,
        claim.start,
        claim.end,
        claim.scale,
        &schedule.events,
    );
    verify_shape(&claim, graphs.len())?;
    let spaces = build_spaces(&claim, &graphs, limits)?;
    verify_nodes(&claim, &graphs, &spaces)?;
    let maps = build_maps(&spaces, schedule.events.len(), claim.modulus)?;
    verify_arrows(&claim, &maps)?;
    verify_decomposition(&claim, &spaces, &maps)?;
    Ok(zigzag_summary(&claim, &spaces, &maps))
}

fn verify_schedule(
    claim: &KineticClaim,
    limits: ProofLimits,
) -> Result<model::ExactSchedule, ProofError> {
    let schedule = exact_schedule(
        &claim.trajectories,
        claim.start,
        claim.end,
        claim.scale,
        limits,
    )?;
    if schedule.persistent_ties != claim.persistent_ties {
        Err(ProofError::new(
            "kinetic zigzag persistent-tie count is wrong",
        ))
    } else {
        Ok(schedule)
    }
}

fn verify_shape(claim: &KineticClaim, graph_count: usize) -> Result<(), ProofError> {
    let maximum_ranks = claim
        .node_ranks
        .len()
        .checked_mul(claim.node_ranks.len())
        .ok_or_else(|| ProofError::new("kinetic zigzag rank count overflows"))?;
    if graph_count != claim.node_ranks.len()
        || claim.node_edges.len() != graph_count
        || claim.arrow_ranks.len() + 1 != graph_count
        || claim.generalized_ranks.len() != maximum_ranks
    {
        Err(ProofError::new("kinetic zigzag claim shape is wrong"))
    } else {
        Ok(())
    }
}

fn build_spaces(
    claim: &KineticClaim,
    graphs: &[Vec<crate::cohomology::Edge>],
    limits: ProofLimits,
) -> Result<Vec<Space>, ProofError> {
    graphs
        .iter()
        .map(|graph| {
            Space::build(
                claim.vertex_count,
                claim.dimension,
                graph,
                claim.modulus,
                limits,
            )
        })
        .collect()
}

fn verify_nodes(
    claim: &KineticClaim,
    graphs: &[Vec<crate::cohomology::Edge>],
    spaces: &[Space],
) -> Result<(), ProofError> {
    let ranks = spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let edges = graphs.iter().map(Vec::len).collect::<Vec<_>>();
    if claim.node_ranks != ranks || claim.node_edges != edges {
        Err(ProofError::new(
            "kinetic zigzag node ranks or active-edge counts are wrong",
        ))
    } else {
        Ok(())
    }
}

fn build_maps(spaces: &[Space], event_count: usize, modulus: u32) -> Result<Vec<Map>, ProofError> {
    let mut maps = Vec::with_capacity(event_count * 2);
    for event in 0..event_count {
        append_event_maps(&mut maps, spaces, event, modulus)?;
    }
    Ok(maps)
}

fn append_event_maps(
    maps: &mut Vec<Map>,
    spaces: &[Space],
    event: usize,
    modulus: u32,
) -> Result<(), ProofError> {
    let left = 2 * event;
    let middle = left + 1;
    let right = left + 2;
    let (left_columns, left_rank) = spaces[middle].restriction_to(&spaces[left], modulus)?;
    maps.push(Map {
        forward: false,
        columns: left_columns,
        rank: left_rank,
    });
    let (right_columns, right_rank) = spaces[middle].restriction_to(&spaces[right], modulus)?;
    maps.push(Map {
        forward: true,
        columns: right_columns,
        rank: right_rank,
    });
    Ok(())
}

fn verify_arrows(claim: &KineticClaim, maps: &[Map]) -> Result<(), ProofError> {
    let ranks = maps.iter().map(|map| map.rank).collect::<Vec<_>>();
    if claim.arrow_ranks != ranks {
        Err(ProofError::new(
            "kinetic zigzag restriction ranks are wrong",
        ))
    } else {
        Ok(())
    }
}

fn verify_decomposition(
    claim: &KineticClaim,
    spaces: &[Space],
    maps: &[Map],
) -> Result<(), ProofError> {
    let ranks = spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let (checked_ranks, checked_intervals) = decompose(&ranks, maps, claim.modulus)?;
    if claim.generalized_ranks != checked_ranks || claim.intervals != checked_intervals {
        Err(ProofError::new(
            "kinetic zigzag interval decomposition is wrong",
        ))
    } else {
        Ok(())
    }
}

fn zigzag_summary(claim: &KineticClaim, spaces: &[Space], maps: &[Map]) -> VerifiedKineticZigzag {
    VerifiedKineticZigzag {
        dimension: claim.dimension,
        modulus: claim.modulus,
        edges: claim.trajectories.len(),
        nodes: spaces.len(),
        arrows: maps.len(),
        intervals: claim.intervals.len(),
        interval_copies: claim.intervals.iter().map(|item| item.2).sum(),
    }
}

pub use model::VerifiedKineticZigzag;
