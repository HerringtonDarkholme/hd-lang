//! The process-wide work signal of the generated scale check (#1880,
//! `bench/metrics/scripts/scale.ts`): one table of named counters, off
//! unless [`start`] turns it on. Every opt-in `work::count_*` site of the
//! compiler feeds its counter here as well as its own thread-local
//! measurement, the table hasher counts each hash it starts, and `hd`'s
//! allocator counts allocations and their bytes. A serial run does the
//! same work in the same order each time, so each counter is
//! deterministic and two input sizes compare without timing noise.
//!
//! Cost: one relaxed load and a predicted branch per site while off; one
//! relaxed add while on. Memory is the fixed table.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

macro_rules! counters {
    ($($variant:ident = $name:literal,)*) => {
        /// One counter of the table, named in the report by
        /// [`Counter::name`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Counter {
            $($variant,)*
        }

        impl Counter {
            /// Every counter, in report order.
            pub const ALL: &'static [Counter] = &[$(Counter::$variant,)*];

            /// The counter's name in the report.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Counter::$variant => $name,)*
                }
            }
        }
    };
}

counters! {
    Allocs = "allocs",
    AllocBytes = "alloc_bytes",
    Hashes = "hashes",
    IndexedBytes = "indexed_bytes",
    ScopeProbes = "scope_probes",
    OuterProbes = "outer_probes",
    LocalBodyChecks = "local_body_checks",
    ItemBodyChecks = "item_body_checks",
    EnvClauses = "env_clauses",
    JoinWork = "join_work",
    HeaderTypes = "header_types",
    HeldTypes = "held_types",
    DecisionRows = "decision_rows",
    SolveWork = "solve_work",
    EnumSteps = "enum_steps",
    TypeSteps = "type_steps",
    Control = "control",
    UseFacts = "use_facts",
    LoanQueries = "loan_queries",
    TraitLookup = "trait_lookup",
    TypeNodes = "type_nodes",
    DeriveWork = "derive_work",
    MemberPairs = "member_pairs",
    LoopScans = "loop_scans",
    SlotPathNodes = "slot_path_nodes",
    RootLookups = "root_lookups",
    InitOrderSteps = "init_order_steps",
    ItemDecodes = "item_decodes",
    EntryLookups = "entry_lookups",
}

static ON: AtomicBool = AtomicBool::new(false);
static COUNTS: [AtomicU64; Counter::ALL.len()] = [const { AtomicU64::new(0) }; Counter::ALL.len()];

/// Adds `n` units to `counter` while the signal is on.
#[inline]
pub fn add(counter: Counter, n: u64) {
    if ON.load(Ordering::Relaxed) {
        COUNTS[counter as usize].fetch_add(n, Ordering::Relaxed);
    }
}

/// Turns the signal on for the rest of the process, from zero.
pub fn start() {
    for count in &COUNTS {
        count.store(0, Ordering::Relaxed);
    }
    ON.store(true, Ordering::Relaxed);
}

/// Runs `f` with the signal off, then restores it: work that is not a
/// function of the program, as the disk cache's file names and its
/// time-bounded maintenance, stays out of the totals. Process-wide, so a
/// measured run is serial (`HD_JOBS=1`): another thread's work in the
/// window is not counted either.
pub fn uncounted<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ON.store(self.0, Ordering::Relaxed);
        }
    }
    let _restore = Restore(ON.swap(false, Ordering::Relaxed));
    f()
}

/// Each counter's name and total so far, in [`Counter::ALL`] order.
#[must_use]
pub fn totals() -> Vec<(&'static str, u64)> {
    Counter::ALL
        .iter()
        .map(|&c| (c.name(), COUNTS[c as usize].load(Ordering::Relaxed)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Counter, add, start, totals};

    #[test]
    fn counts_only_after_start_and_names_every_counter_once() {
        // One test owns the process-wide table, so the order is fixed.
        add(Counter::Hashes, 5);
        start();
        add(Counter::Control, 2);
        super::uncounted(|| add(Counter::Control, 7));
        add(Counter::Control, 3);
        let mut set = crate::HashSet::default();
        set.insert(1u32);
        let all = totals();
        assert_eq!(all.len(), Counter::ALL.len());
        let names: std::collections::BTreeSet<_> = all.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.len(), all.len());
        assert!(all.contains(&("control", 5)));
        // The table hasher counts each hash it starts; other tests running
        // beside this one may add theirs.
        assert!(all.iter().any(|&(n, c)| n == "hashes" && c >= 1));
    }
}
