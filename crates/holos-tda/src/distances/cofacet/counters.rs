/// Untimed event counters for the sparse cofacet enumerator.
///
/// Only a test build has them. Elsewhere the `note_*` functions are empty.
/// The counts are thread-local because the test binary runs tests on
/// several threads at once.
#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static CANDIDATES: Cell<u64> = const { Cell::new(0) };
    static CALLBACKS: Cell<u64> = const { Cell::new(0) };
    static BREAKS: Cell<u64> = const { Cell::new(0) };
    static SPILLS: Cell<u64> = const { Cell::new(0) };
    static VACUOUS: Cell<u64> = const { Cell::new(0) };
}

/// What one thread's enumerations did since the last [`reset`].
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Events {
    /// Neighbor-list entries the enumerator read.
    pub(crate) candidates: u64,
    /// Calls the enumerator made to the caller's closure.
    pub(crate) callbacks: u64,
    /// Breaks the enumerator honored.
    pub(crate) breaks: u64,
    /// Enumerations that allocated their cursors instead of keeping
    /// them on the stack.
    pub(crate) spills: u64,
    /// Bounded walks whose bound rose to infinity, because no stored
    /// distance reached it.
    pub(crate) vacuous: u64,
}

#[cfg(test)]
#[inline]
pub(crate) fn note_candidate() {
    CANDIDATES.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
#[inline]
pub(crate) fn note_callback() {
    CALLBACKS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
#[inline]
pub(crate) fn note_break() {
    BREAKS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
#[inline]
pub(crate) fn note_spill() {
    SPILLS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
#[inline]
pub(crate) fn note_vacuous_bound() {
    VACUOUS.with(|c| c.set(c.get() + 1));
}

/// Zero this thread's counters.
#[cfg(test)]
pub(crate) fn reset() {
    CANDIDATES.with(|c| c.set(0));
    CALLBACKS.with(|c| c.set(0));
    BREAKS.with(|c| c.set(0));
    SPILLS.with(|c| c.set(0));
    VACUOUS.with(|c| c.set(0));
}

/// Read this thread's counters.
#[cfg(test)]
pub(crate) fn read() -> Events {
    Events {
        candidates: CANDIDATES.with(Cell::get),
        callbacks: CALLBACKS.with(Cell::get),
        breaks: BREAKS.with(Cell::get),
        spills: SPILLS.with(Cell::get),
        vacuous: VACUOUS.with(Cell::get),
    }
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn note_candidate() {}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn note_callback() {}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn note_break() {}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn note_spill() {}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn note_vacuous_bound() {}
