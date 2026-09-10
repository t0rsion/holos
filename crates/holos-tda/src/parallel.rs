//! Concurrent column reduction (giotto-ph / Morozov-Nigmetov style). Worker
//! threads reduce columns out of order. They compete for pivots through a
//! shared table under column-order priority. A column whose pivot is held by
//! a larger-indexed column evicts that owner and re-queues it. A column that
//! finds a smaller-indexed owner reduces against it. The table converges to
//! the unique reduced pivot set. The barcode is identical to the serial
//! engine at every thread count.
//!
//! Each pivot's V-column lives in its table entry, so a reader observes an
//! owner and its V-column as one consistent snapshot. All coboundary
//! arithmetic reuses the `&self` methods in [`crate::reduce`].

use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry as MapEntry;
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::Bar;
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::reduce::{Engine, PairScratch, Pivots};
use crate::simplex::Simplex;

/// A reduced column that owns a pivot: the pivot's coefficient and diameter,
/// the owning column, and the column's V-column (the reducers combined into
/// it, its own leading term implicit).
#[derive(Clone)]
struct Owner {
    coeff: u64,
    diameter: f64,
    col: usize,
    v: Arc<[Entry]>,
}

/// The shared pivot table: pivot index -> owner. `DashMap` shards over an
/// `RwLock` per shard, so the frequent reads (`get`/`contains`) run in
/// parallel and only claims take a write lock.
type Table = DashMap<u64, Owner, FxBuildHasher>;

/// Outcome of trying to install a column as the owner of a pivot.
enum Claim {
    /// The pivot was unowned; this column now owns it.
    Won,
    /// This column displaced a larger-indexed owner, which must re-reduce.
    Displaced(usize),
    /// A smaller-indexed column owns the pivot; re-reduce against it.
    Lost,
}

/// Install `owner` as the owner of `index` under column-order priority. The
/// smaller column index wins and evicts a larger incumbent.
fn claim(table: &Table, index: u64, owner: Owner) -> Claim {
    match table.entry(index) {
        MapEntry::Vacant(slot) => {
            slot.insert(owner);
            Claim::Won
        }
        MapEntry::Occupied(mut slot) => {
            let incumbent = slot.get().col;
            if incumbent < owner.col {
                Claim::Lost
            } else {
                slot.insert(owner);
                Claim::Displaced(incumbent)
            }
        }
    }
}

/// One field on a cache line of its own. Two workers that write two
/// separate counters must not share a line, or each write costs the other a
/// miss. The alignment covers the adjacent-line prefetch as well.
#[repr(align(128))]
struct Padded<T>(T);

/// Idle polls a worker spends yielding before it sleeps.
const IDLE_YIELDS: u32 = 32;
/// The sleep doubles from one microsecond this many times, then holds.
const IDLE_SLEEP_DOUBLINGS: u32 = 7;

/// Columns awaiting reduction. An atomic counter dispenses the initial
/// `0..len` lock-free. A mutex holds the rare displaced columns to
/// re-reduce, and a flag gates that mutex, so the common path never locks
/// it.
///
/// The three shared counters sit on separate cache lines. A worker writes
/// `next` on every column, so a `pending` on the same line would cost every
/// other worker a miss per column.
struct WorkQueue {
    /// The next undispensed column.
    next: Padded<AtomicUsize>,
    /// Columns not yet in a final state; the region ends when it hits zero.
    pending: Padded<AtomicUsize>,
    has_requeued: Padded<AtomicBool>,
    requeued: Mutex<Vec<usize>>,
    len: usize,
}

impl WorkQueue {
    fn new(len: usize) -> Self {
        Self {
            next: Padded(AtomicUsize::new(0)),
            pending: Padded(AtomicUsize::new(len)),
            has_requeued: Padded(AtomicBool::new(false)),
            requeued: Mutex::new(Vec::new()),
            len,
        }
    }

    /// The next column to reduce. A displaced column comes first, so a
    /// re-reduction does not wait behind the undispensed columns.
    fn take(&self) -> Option<usize> {
        if self.has_requeued.0.load(Ordering::Acquire) {
            if let Some(col) = self.pop_requeued() {
                return Some(col);
            }
        }
        // Saturate at `len`. An unbounded counter could wrap on a long run
        // and dispense a column twice.
        self.next
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |i| {
                (i < self.len).then_some(i + 1)
            })
            .ok()
    }

    fn pop_requeued(&self) -> Option<usize> {
        let mut queue = self.requeued.lock().unwrap();
        let col = queue.pop();
        if queue.is_empty() {
            self.has_requeued.0.store(false, Ordering::Release);
        }
        col
    }

    fn requeue(&self, col: usize) {
        // Set the flag under the lock, so a worker that observes it always
        // sees the pushed column. `pending` (already incremented by the
        // caller) keeps a worker alive to re-poll.
        let mut queue = self.requeued.lock().unwrap();
        queue.push(col);
        self.has_requeued.0.store(true, Ordering::Release);
    }
}

/// Read-only context shared by every worker for one dimension.
struct Ctx<'a> {
    columns: &'a [Simplex],
    dim: usize,
    prev_pivots: &'a Pivots,
    table: &'a Table,
    queue: &'a WorkQueue,
    /// One shared allocation reused for every empty V-column.
    empty_v: Arc<[Entry]>,
}

/// Per-worker mutable scratch, reused across the columns a worker processes.
#[derive(Default)]
struct Scratch {
    working_cob: BinaryHeap<HeapEntry>,
    working_red: BinaryHeap<HeapEntry>,
    cofacet_buf: Vec<Entry>,
    v_buf: Vec<Entry>,
    verts: Vec<usize>,
    cofacet_verts: Vec<usize>,
    pairs: PairScratch,
}

/// The state in which one reduction pass over a column ended.
enum Pass {
    /// Owns a pivot; carries a column it displaced (to re-reduce), if any.
    Owned(Option<usize>),
    /// Reduced to zero: an essential class (unless cleared as a prior death).
    Essential,
    /// A smaller column claimed this pivot mid-flight; re-run the whole pass.
    Requeue,
}

impl<C: Coeffs + Sync, D: Distances + Sync> Engine<'_, C, D> {
    /// Parallel counterpart of [`Engine::reduce_dimension`]: same pivot
    /// registry, with bars returned rather than pushed. Runs `budget`
    /// workers on the engine's run-wide worker pool. The budget is at most
    /// the pool size, so the pool threads above it return at once.
    pub(crate) fn reduce_dimension_parallel(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        budget: usize,
    ) -> (Pivots, Vec<Bar>) {
        if columns.is_empty() {
            return (FxHashMap::default(), Vec::new());
        }
        let (table, bars) = self.converge(columns, dim, prev_pivots, budget);
        (self.to_pivots(&table), bars)
    }

    /// Run the workers until the table holds the reduced pivot set, then
    /// read the dimension's bars off it.
    fn converge(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        budget: usize,
    ) -> (Table, Vec<Bar>) {
        let table: Table = DashMap::with_capacity_and_hasher(columns.len(), FxBuildHasher);
        let queue = WorkQueue::new(columns.len());
        let ctx = Ctx {
            columns,
            dim,
            prev_pivots,
            table: &table,
            queue: &queue,
            empty_v: Arc::from(Vec::new()),
        };

        self.install(|| {
            rayon::broadcast(|worker| {
                if worker.index() < budget {
                    let mut scratch = Scratch::default();
                    self.worker(&ctx, &mut scratch);
                }
            });
        });

        let bars = self.collect_bars(&ctx);
        drop(ctx);
        (table, bars)
    }

    /// The converged pivot registry, with the pivot diameter the reduced
    /// output drops: (pivot index, coefficient, owner column, diameter
    /// bits), by pivot index. The thread-invariance gate compares this,
    /// which is wider than the diagram.
    #[cfg(test)]
    pub(crate) fn parallel_pivot_registry(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        budget: usize,
    ) -> Vec<(u64, u64, usize, u64)> {
        let (table, _) = self.converge(columns, dim, prev_pivots, budget);
        let mut registry: Vec<(u64, u64, usize, u64)> = table
            .iter()
            .map(|r| {
                let owner = r.value();
                (*r.key(), owner.coeff, owner.col, owner.diameter.to_bits())
            })
            .collect();
        registry.sort_unstable();
        registry
    }

    // `inline(never)` keeps ThinLTO's inliner from folding the whole reduce
    // pass into the rayon broadcast closure. That fold overflows the
    // inliner's recursion.
    /// A worker counts its finished columns privately and subtracts them
    /// from `pending` only when the queue runs dry. That takes the busiest
    /// shared write off the per-column path. The count is always reported
    /// before the worker reads `pending`, and a displacement raises
    /// `pending` while the displacing column is still counted there, so
    /// `pending` never reads zero with a column still in flight.
    #[inline(never)]
    fn worker(&self, ctx: &Ctx, scratch: &mut Scratch) {
        let mut done = 0usize;
        let mut idle = 0u32;
        loop {
            let Some(col) = ctx.queue.take() else {
                if done > 0 {
                    ctx.queue.pending.0.fetch_sub(done, Ordering::AcqRel);
                    done = 0;
                }
                if ctx.queue.pending.0.load(Ordering::Acquire) == 0 {
                    return;
                }
                // The queue is dry but a column is still in flight, and it
                // may come back displaced. Yield first, then sleep for
                // longer each time, so a worker with nothing left stops
                // reading the shared counters in a tight loop.
                if idle < IDLE_YIELDS {
                    std::thread::yield_now();
                } else {
                    let step = (idle - IDLE_YIELDS).min(IDLE_SLEEP_DOUBLINGS);
                    std::thread::sleep(std::time::Duration::from_micros(1 << step));
                }
                idle += 1;
                continue;
            };
            idle = 0;
            match self.reduce_pass(ctx, scratch, col) {
                Pass::Owned(displaced) => {
                    if let Some(k) = displaced {
                        ctx.queue.pending.0.fetch_add(1, Ordering::AcqRel);
                        ctx.queue.requeue(k);
                    }
                    done += 1;
                }
                Pass::Essential => done += 1,
                Pass::Requeue => ctx.queue.requeue(col),
            }
        }
    }

    /// Reduce column `col` once against the current table.
    fn reduce_pass(&self, ctx: &Ctx, scratch: &mut Scratch, col: usize) -> Pass {
        let column = ctx.columns[col];
        scratch.working_cob.clear();
        scratch.working_red.clear();
        let mut pivot = self.init_coboundary(
            column,
            ctx.dim,
            |index| ctx.table.contains_key(&index),
            &mut scratch.working_cob,
            &mut scratch.cofacet_buf,
            &mut scratch.verts,
            &mut scratch.cofacet_verts,
            &mut scratch.pairs,
        );
        // The emergent shortcut returns a pivot without building the working
        // column. A column that then has to reduce must build it first.
        let mut built = !(pivot.is_some() && scratch.working_cob.is_empty());

        loop {
            let Some(p) = pivot else {
                return Pass::Essential;
            };
            let index = self.ops.index(p);
            // Clone out of the guard and drop it: holding a DashMap read guard
            // while claiming (a write on the same shard) would self-deadlock.
            let held = ctx.table.get(&index).map(|r| r.clone());
            match held {
                Some(owner) if owner.col < col => {
                    if !built {
                        self.build_full_coboundary(
                            column,
                            ctx.dim,
                            &mut scratch.working_cob,
                            &mut scratch.verts,
                        );
                        built = true;
                    }
                    self.fold_reducer(
                        p,
                        owner.coeff,
                        ctx.columns[owner.col],
                        &owner.v,
                        ctx.dim,
                        &mut scratch.working_red,
                        &mut scratch.working_cob,
                        &mut scratch.verts,
                    );
                    pivot = self.get_pivot(&mut scratch.working_cob);
                }
                _ => {
                    if let Some(next) = self.reduce_apparent_facet(
                        p,
                        ctx.dim,
                        &mut scratch.working_red,
                        &mut scratch.working_cob,
                        &mut scratch.verts,
                        &mut scratch.pairs,
                    ) {
                        pivot = next;
                    } else {
                        return self.claim_pivot(ctx, scratch, p, index, col);
                    }
                }
            }
        }
    }

    /// Drain the reduced column into a V-column and install it as the owner of
    /// `index` under column-order priority.
    fn claim_pivot(
        &self,
        ctx: &Ctx,
        scratch: &mut Scratch,
        p: Entry,
        index: u64,
        col: usize,
    ) -> Pass {
        scratch.v_buf.clear();
        self.drain_into(&mut scratch.working_red, &mut scratch.v_buf);
        let v = if scratch.v_buf.is_empty() {
            ctx.empty_v.clone()
        } else {
            Arc::from(scratch.v_buf.as_slice())
        };
        let owner = Owner {
            coeff: self.ops.coeff(p),
            diameter: p.diameter,
            col,
            v,
        };
        match claim(ctx.table, index, owner) {
            Claim::Won => Pass::Owned(None),
            Claim::Displaced(k) => Pass::Owned(Some(k)),
            Claim::Lost => Pass::Requeue,
        }
    }

    fn to_pivots(&self, table: &Table) -> Pivots {
        table
            .iter()
            .map(|r| {
                let owner = r.value();
                (*r.key(), (owner.coeff, owner.col))
            })
            .collect()
    }

    /// Derive the dimension's bars from the converged table: a finite bar per
    /// owned pivot, an essential bar per unowned column that is not a prior
    /// death.
    fn collect_bars(&self, ctx: &Ctx) -> Vec<Bar> {
        let mut bars = Vec::new();
        let mut owns_pivot = vec![false; ctx.columns.len()];
        for r in ctx.table.iter() {
            let owner = r.value();
            owns_pivot[owner.col] = true;
            let birth = ctx.columns[owner.col].diameter;
            if owner.diameter > birth {
                bars.push(Bar {
                    dim: ctx.dim,
                    birth,
                    death: owner.diameter,
                });
            }
        }
        for (col, &owned) in owns_pivot.iter().enumerate() {
            let column = ctx.columns[col];
            let prior_death =
                !self.params.use_clearing && ctx.prev_pivots.contains_key(&column.index);
            if !owned && !prior_death {
                bars.push(Bar {
                    dim: ctx.dim,
                    birth: column.diameter,
                    death: f64::INFINITY,
                });
            }
        }
        bars
    }
}

#[cfg(test)]
mod tests;
