use sha2::{Digest, Sha256};

use crate::certificate::CertificateLimits;
use crate::circular::IntegralCocycleTerm;

use super::{
    F64_BITS_CODEC, MAGIC, PersistentCoordinateArtifact, PersistentCoordinateArtifactError, VERSION,
};

impl PersistentCoordinateArtifact {
    /// Encode the bounded canonical `HOLOSPH` version 1 envelope.
    ///
    /// Resource limits are checked for every call. The nested class and
    /// selected coordinate were validated when their private fields were built.
    pub fn encode(
        &self,
        limits: CertificateLimits,
    ) -> Result<Vec<u8>, PersistentCoordinateArtifactError> {
        let class = self
            .class_artifact
            .encode(limits)
            .map_err(|error| artifact_error(error.to_string()))?;
        validate_encode_limits(self, class.len(), limits)?;
        let payload_len =
            encoded_payload_len(class.len(), self.integral().len(), self.potential().len())?;
        validate_encoded_length(payload_len, limits)?;
        let mut output = Vec::with_capacity(payload_len);
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        put_u64(&mut output, self.tolerance().to_bits());
        put_usize(&mut output, class.len(), "nested class artifact byte count")?;
        output.extend_from_slice(&class);
        put_u32(&mut output, self.field_multiplier());
        put_u64(&mut output, self.divisibility());
        put_usize(
            &mut output,
            self.integral().len(),
            "integral lift term count",
        )?;
        encode_integral(&mut output, self.integral())?;
        put_usize(&mut output, self.potential().len(), "potential count")?;
        for &value in self.potential() {
            put_u64(&mut output, value.to_bits());
        }
        debug_assert_eq!(output.len(), payload_len);
        let digest: [u8; 32] = Sha256::digest(&output).into();
        output.extend_from_slice(&digest);
        Ok(output)
    }
}

fn encode_integral(
    output: &mut Vec<u8>,
    terms: &[IntegralCocycleTerm],
) -> Result<(), PersistentCoordinateArtifactError> {
    for term in terms {
        put_usize(output, term.u, "integral lift endpoint")?;
        put_usize(output, term.v, "integral lift endpoint")?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn validate_encode_limits(
    artifact: &PersistentCoordinateArtifact,
    class_bytes: usize,
    limits: CertificateLimits,
) -> Result<(), PersistentCoordinateArtifactError> {
    if class_bytes > limits.max_bytes {
        return Err(artifact_error(
            "nested persistent class artifact exceeds its byte limit",
        ));
    }
    if artifact.source().len() > limits.max_vertices {
        return Err(artifact_error(
            "persistent coordinate source exceeds the vertex limit",
        ));
    }
    if artifact.integral().len() > limits.max_terms {
        return Err(artifact_error(
            "persistent coordinate lift exceeds the term limit",
        ));
    }
    if artifact.potential().len() > limits.max_vertices {
        return Err(artifact_error(
            "persistent coordinate potential exceeds the vertex limit",
        ));
    }
    Ok(())
}

fn encoded_payload_len(
    class_bytes: usize,
    integral_terms: usize,
    potential_values: usize,
) -> Result<usize, PersistentCoordinateArtifactError> {
    let fixed: usize = 8 + 2 + 1 + 8 + 8 + 4 + 8 + 8 + 8;
    let integral_bytes = integral_terms
        .checked_mul(24)
        .ok_or_else(|| artifact_error("persistent coordinate artifact byte count overflows"))?;
    let potential_bytes = potential_values
        .checked_mul(8)
        .ok_or_else(|| artifact_error("persistent coordinate artifact byte count overflows"))?;
    fixed
        .checked_add(class_bytes)
        .and_then(|length| length.checked_add(integral_bytes))
        .and_then(|length| length.checked_add(potential_bytes))
        .ok_or_else(|| artifact_error("persistent coordinate artifact byte count overflows"))
}

fn validate_encoded_length(
    payload_len: usize,
    limits: CertificateLimits,
) -> Result<(), PersistentCoordinateArtifactError> {
    let total = payload_len
        .checked_add(32)
        .ok_or_else(|| artifact_error("persistent coordinate artifact byte count overflows"))?;
    if total > limits.max_bytes {
        return Err(artifact_error(
            "persistent coordinate artifact exceeds its byte limit",
        ));
    }
    Ok(())
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
) -> Result<(), PersistentCoordinateArtifactError> {
    let value =
        u64::try_from(value).map_err(|_| artifact_error(format!("{name} does not fit in u64")))?;
    put_u64(output, value);
    Ok(())
}

fn artifact_error(message: impl Into<String>) -> PersistentCoordinateArtifactError {
    PersistentCoordinateArtifactError::new(message)
}
