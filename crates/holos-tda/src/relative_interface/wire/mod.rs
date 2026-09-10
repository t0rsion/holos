mod decode;
mod encode;

use crate::certificate::{CertificateError, CertificateLimits};

use super::model::RelativeInterfaceCertificate;

pub(super) const MAGIC: &[u8; 8] = b"HOLOSRI\0";
pub(super) const VERSION: u16 = 1;
pub(super) const F64_BITS_CODEC: u8 = 1;

impl RelativeInterfaceCertificate {
    /// Encode the canonical `HOLOSRI` version 1 certificate.
    pub fn encode(&self, limits: CertificateLimits) -> Result<Vec<u8>, CertificateError> {
        encode::encode(self, limits)
    }

    /// Decode and verify one bounded `HOLOSRI` version 1 certificate.
    pub fn decode(bytes: &[u8], limits: CertificateLimits) -> Result<Self, CertificateError> {
        decode::decode(bytes, limits)
    }
}
