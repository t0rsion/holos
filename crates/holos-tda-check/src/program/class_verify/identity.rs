use sha2::{Digest, Sha256};

use crate::proof::{Graph, ProofBar};

use super::super::claim::{CocycleTermClaim, CriticalPairClaim};
use super::super::reduction::H1Pair;

pub(super) fn critical_pair_order(
    birth: &[usize],
    death: Option<&[usize]>,
    pair: &CriticalPairClaim,
) -> std::cmp::Ordering {
    birth
        .cmp(&pair.birth.vertices)
        .then_with(|| match (death, pair.death.as_ref()) {
            (Some(a), Some(b)) => a.cmp(&b.vertices),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

pub(super) fn declared_pair_order(
    left: &(ProofBar, CriticalPairClaim),
    right: &(ProofBar, CriticalPairClaim),
) -> std::cmp::Ordering {
    left.0
        .birth
        .total_cmp(&right.0.birth)
        .then(left.0.death.total_cmp(&right.0.death))
        .then(left.1.birth.vertices.cmp(&right.1.birth.vertices))
        .then_with(|| match (&left.1.death, &right.1.death) {
            (Some(a), Some(b)) => a.vertices.cmp(&b.vertices),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

pub(super) fn pair_order(left: &H1Pair, right: &H1Pair) -> std::cmp::Ordering {
    left.interval
        .birth
        .total_cmp(&right.interval.birth)
        .then(left.interval.death.total_cmp(&right.interval.death))
        .then(left.birth.cmp(&right.birth))
        .then_with(|| match (&left.death, &right.death) {
            (Some(a), Some(b)) => a.cmp(b),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

pub(super) fn terminal_level(graph: &Graph, threshold: Option<f64>) -> f64 {
    let maximum = graph
        .edges
        .iter()
        .map(|edge| edge.value)
        .fold(0.0, f64::max);
    threshold.map_or(maximum, |threshold| threshold.min(maximum))
}

pub(super) fn previous_float(value: f64) -> f64 {
    f64::from_bits(value.to_bits() - 1)
}

pub(super) fn bar_bits_equal(left: ProofBar, right: ProofBar) -> bool {
    left.dimension == right.dimension
        && left.birth.to_bits() == right.birth.to_bits()
        && left.death.to_bits() == right.death.to_bits()
}

pub(super) fn valid_interval_and_scale(interval: ProofBar, scale: f64) -> bool {
    interval.dimension == 1
        && valid_nonnegative_finite(interval.birth)
        && valid_death(interval.death)
        && (interval.death.is_infinite() || interval.death > interval.birth)
        && valid_nonnegative_finite(scale)
        && scale >= interval.birth
        && (interval.death.is_infinite() || scale < interval.death)
}

fn valid_nonnegative_finite(value: f64) -> bool {
    value.is_finite() && value >= 0.0 && !is_negative_zero(value)
}

fn valid_death(value: f64) -> bool {
    (value.is_finite() && value >= 0.0 && !is_negative_zero(value))
        || (value.is_infinite() && value.is_sign_positive())
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

pub(crate) fn group_id(
    interval: ProofBar,
    modulus: u32,
    scale: f64,
    basis: &[Vec<CocycleTermClaim>],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-class-space-v1");
    hash.update((interval.dimension as u64).to_be_bytes());
    hash.update(interval.birth.to_bits().to_be_bytes());
    hash.update(interval.death.to_bits().to_be_bytes());
    hash.update(modulus.to_be_bytes());
    hash.update((basis.len() as u64).to_be_bytes());
    for terms in basis {
        hash.update(scale.to_bits().to_be_bytes());
        hash.update((terms.len() as u64).to_be_bytes());
        for term in terms {
            hash.update((term.u as u64).to_be_bytes());
            hash.update((term.v as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
    hash.finalize().into()
}

pub(crate) fn basis_class_id(
    group: [u8; 32],
    basis_index: usize,
    scale: f64,
    terms: &[CocycleTermClaim],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-h1-basis-class-v1");
    hash.update(group);
    hash.update((basis_index as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    for term in terms {
        hash.update((term.u as u64).to_be_bytes());
        hash.update((term.v as u64).to_be_bytes());
        hash.update(term.coefficient.to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn source_graph_digest(graph: &Graph, scale: f64) -> [u8; 32] {
    let active: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= scale)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-persistent-class-source-v1");
    hash.update((graph.vertex_count as u64).to_be_bytes());
    hash.update(scale.to_bits().to_be_bytes());
    hash.update((active.len() as u64).to_be_bytes());
    for edge in active {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(crate) fn same_optional_f64(left: Option<f64>, right: Option<f64>) -> bool {
    left.map(f64::to_bits) == right.map(f64::to_bits)
}

#[cfg(test)]
mod tests {
    use super::terminal_level;
    use crate::proof::{Graph, ProofEdge};

    #[test]
    fn terminal_level_caps_an_explicit_threshold_at_the_graph_maximum() {
        let graph = Graph::new(
            3,
            &[
                ProofEdge {
                    u: 0,
                    v: 1,
                    value: 1.0,
                },
                ProofEdge {
                    u: 1,
                    v: 2,
                    value: 5.0,
                },
            ],
        )
        .unwrap();
        assert_eq!(terminal_level(&graph, Some(3.0)), 3.0);
        assert_eq!(terminal_level(&graph, Some(7.0)), 5.0);
        assert_eq!(terminal_level(&graph, None), 5.0);
    }
}
