use crate::ProofError;

use super::claims::WeightedEdge;

pub(crate) fn validate_axes(
    vertex_count: usize,
    threshold_bits: u64,
    scale_bits: &[u64],
    minimum_degrees: &[usize],
) -> Result<Vec<f64>, ProofError> {
    if !has_terminal_scale(scale_bits, threshold_bits) {
        return Err(ProofError::new(
            "the bipersistence scale axis must end at its threshold",
        ));
    }
    let mut scales = Vec::with_capacity(scale_bits.len());
    for &bits in scale_bits {
        let scale = f64::from_bits(bits);
        if !is_canonical_scale(scale, bits, scales.last()) {
            return Err(ProofError::new(
                "bipersistence scales are not canonical and strictly increasing",
            ));
        }
        scales.push(scale);
    }
    if !is_canonical_density_axis(vertex_count, minimum_degrees) {
        return Err(ProofError::new(
            "bipersistence minimum degrees do not form a terminal descending axis",
        ));
    }
    Ok(scales)
}

fn has_terminal_scale(scale_bits: &[u64], threshold_bits: u64) -> bool {
    !scale_bits.is_empty() && scale_bits.last().copied() == Some(threshold_bits)
}

fn is_canonical_scale(scale: f64, bits: u64, previous: Option<&f64>) -> bool {
    scale.is_finite()
        && scale >= 0.0
        && (scale != 0.0 || bits == 0)
        && previous.is_none_or(|previous| *previous < scale)
}

fn is_canonical_density_axis(vertex_count: usize, minimum_degrees: &[usize]) -> bool {
    minimum_degrees.last().copied() == Some(0)
        && !minimum_degrees.windows(2).any(|pair| pair[0] <= pair[1])
        && !minimum_degrees
            .iter()
            .any(|&degree| degree >= vertex_count.max(1))
}

pub(crate) fn degree_table(
    vertex_count: usize,
    edges: &[WeightedEdge],
    scales: &[f64],
) -> Vec<Vec<usize>> {
    scales
        .iter()
        .map(|&scale| {
            let mut degrees = vec![0usize; vertex_count];
            for edge in edges {
                if f64::from_bits(edge.value_bits) <= scale {
                    degrees[edge.u] += 1;
                    degrees[edge.v] += 1;
                }
            }
            degrees
        })
        .collect()
}
