#[path = "fixtures/base.rs"]
mod base;
#[path = "fixtures/batteries.rs"]
mod batteries;
#[path = "fixtures/checks.rs"]
mod checks;

pub(super) use base::*;
pub(super) use batteries::*;
pub(super) use checks::*;
