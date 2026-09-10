//! Edge collapse gates: bar-for-bar equivalence with the uncollapsed engine,
//! certificate properties, adversarial fixtures, and the public API surface.
//!
//! The collapse is preprocessing, so no bar may move by one bit. Every
//! diagram comparison here is exact on canonicalized bars, with no tolerance.
//! Small fixtures also face the brute-force oracle.
//!
//! The hand-built fixtures name the property they attack. Their edge sets are
//! chosen against the frozen schedule (decreasing value, ties by combinadic
//! index) and the frozen witness rule, so the expected certificates below are
//! derived from the specification, not observed from a run.

#[path = "collapse/adversarial.rs"]
mod adversarial;
#[path = "collapse/battery.rs"]
mod battery;
mod common;
#[path = "collapse/fixtures.rs"]
mod fixtures;
#[path = "collapse/large.rs"]
mod large;
#[path = "collapse/reference.rs"]
mod reference;
#[path = "collapse/schedules.rs"]
mod schedules;
