use std::fmt;

/// A failed certificate check: which step failed, when one did, and what
/// rule it broke.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyError {
    /// 0-based index of the failing removal step; `None` for header,
    /// output, or fixed-point failures.
    pub step: Option<usize>,
    /// What was violated.
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.step {
            Some(i) => write!(f, "certificate step {i}: {}", self.message),
            None => write!(f, "certificate: {}", self.message),
        }
    }
}

impl std::error::Error for VerifyError {}

pub(super) fn fail(step: Option<usize>, message: impl Into<String>) -> VerifyError {
    VerifyError {
        step,
        message: message.into(),
    }
}
