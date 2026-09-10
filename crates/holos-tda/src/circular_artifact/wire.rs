//! Canonical circular artifact encoding.

use sha2::{Digest, Sha256};

use crate::{CircularClassTerm, CircularCoordinate, CohomologyContinuationKind};

use super::{
    ArtifactContinuation, ArtifactState, CircularArtifactError, CircularCoordinateArtifact,
    F64_BITS_CODEC, MAGIC, VERSION,
};

impl CircularCoordinateArtifact {
    /// Encode the canonical `HOLOSCC` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, CircularArtifactError> {
        if self.states.is_empty() || self.states.len() > 2 {
            return Err(CircularArtifactError::new(
                "artifact needs one state or one continuation pair",
            ));
        }
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        put_u32(&mut output, self.modulus);
        put_f64(&mut output, self.scale);
        put_f64(&mut output, self.tolerance);
        put_usize(&mut output, self.states.len(), "state count")?;
        for state in &self.states {
            encode_state(&mut output, state)?;
        }
        match &self.continuation {
            None => output.push(0),
            Some(continuation) => {
                output.push(1);
                encode_continuation(&mut output, continuation)?;
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"holos-circular-coordinate-v1");
        hash.update(&output);
        output.extend_from_slice(&hash.finalize());
        Ok(output)
    }
}

fn encode_state(
    output: &mut Vec<u8>,
    state: &ArtifactState,
) -> std::result::Result<(), CircularArtifactError> {
    put_usize(output, state.vertex_count, "vertex count")?;
    put_usize(output, state.edges.len(), "edge count")?;
    for &(u, v) in &state.edges {
        put_usize(output, u, "edge endpoint")?;
        put_usize(output, v, "edge endpoint")?;
    }
    match &state.coordinate {
        None => output.push(0),
        Some(coordinate) => encode_coordinate(output, coordinate)?,
    }
    Ok(())
}

fn encode_coordinate(
    output: &mut Vec<u8>,
    coordinate: &CircularCoordinate,
) -> std::result::Result<(), CircularArtifactError> {
    output.push(1);
    output.extend_from_slice(coordinate.space.as_bytes());
    put_u32(output, coordinate.field_multiplier);
    put_u64(output, coordinate.divisibility);
    put_usize(output, coordinate.source.len(), "source term count")?;
    put_usize(output, coordinate.integral.len(), "integral term count")?;
    put_usize(output, coordinate.potential.len(), "potential count")?;
    encode_source(output, coordinate)?;
    encode_integral(output, coordinate)?;
    encode_class_terms(output, &coordinate.class)?;
    for &value in &coordinate.potential {
        put_f64(output, value);
    }
    Ok(())
}

fn encode_source(
    output: &mut Vec<u8>,
    coordinate: &CircularCoordinate,
) -> std::result::Result<(), CircularArtifactError> {
    for term in &coordinate.source {
        put_usize(output, term.u, "source endpoint")?;
        put_usize(output, term.v, "source endpoint")?;
        put_u32(output, term.coefficient);
    }
    Ok(())
}

fn encode_integral(
    output: &mut Vec<u8>,
    coordinate: &CircularCoordinate,
) -> std::result::Result<(), CircularArtifactError> {
    for term in &coordinate.integral {
        put_usize(output, term.u, "integral endpoint")?;
        put_usize(output, term.v, "integral endpoint")?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn encode_continuation(
    output: &mut Vec<u8>,
    continuation: &ArtifactContinuation,
) -> std::result::Result<(), CircularArtifactError> {
    output.push(match continuation.kind {
        CohomologyContinuationKind::Unique => 1,
        CohomologyContinuationKind::Ambiguous => 2,
        CohomologyContinuationKind::NoExtension => 3,
        CohomologyContinuationKind::NoNonzeroContinuation => 4,
    });
    encode_class_terms(output, &continuation.target)?;
    put_usize(output, continuation.ambiguity.len(), "ambiguity row count")?;
    for row in &continuation.ambiguity {
        encode_class_terms(output, row)?;
    }
    Ok(())
}

fn encode_class_terms(
    output: &mut Vec<u8>,
    terms: &[CircularClassTerm],
) -> std::result::Result<(), CircularArtifactError> {
    put_usize(output, terms.len(), "class term count")?;
    for term in terms {
        put_usize(output, term.basis_index, "class basis index")?;
        put_u32(output, term.coefficient);
    }
    Ok(())
}

fn put_f64(output: &mut Vec<u8>, value: f64) {
    put_u64(output, value.to_bits());
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    output: &mut Vec<u8>,
    value: usize,
    name: &str,
) -> std::result::Result<(), CircularArtifactError> {
    put_u64(
        output,
        u64::try_from(value)
            .map_err(|_| CircularArtifactError::new(format!("{name} does not fit u64")))?,
    );
    Ok(())
}
