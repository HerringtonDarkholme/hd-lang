//! Per-stage tallies (the architecture view of a run) and run counters.

use std::collections::BTreeMap;

use hd_base::{Hash128, Stage};

/// One stage's tally.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub ok: usize,
    pub not_implemented: usize,
    pub blocked: usize,
    pub first_reason: Option<String>,
}

/// Per-stage counts for one package.
#[derive(Clone, Debug, Default)]
pub struct PipelineReport {
    pub package: String,
    pub stages: BTreeMap<Stage, Tally>,
    /// Not-implemented reasons by frequency (first 90 characters).
    pub reasons: BTreeMap<String, usize>,
    /// Function bodies checked, and bodies that stopped at a construct the
    /// checker does not carry, with those reasons.
    pub body_ok: usize,
    pub body_failed: usize,
    pub body_reasons: BTreeMap<String, usize>,
    pub body_failures: Vec<String>,
}

impl PipelineReport {
    pub(crate) fn ok(&mut self, s: Stage) {
        self.stages.entry(s).or_default().ok += 1;
    }
    pub(crate) fn blocked(&mut self, s: Stage) {
        self.stages.entry(s).or_default().blocked += 1;
    }
    pub(crate) fn not_implemented(&mut self, s: Stage, what: &str) {
        let t = self.stages.entry(s).or_default();
        t.not_implemented += 1;
        if t.first_reason.is_none() {
            t.first_reason = Some(what.to_owned());
        }
        let short: String = format!("{}: {what}", s.name()).chars().take(90).collect();
        *self.reasons.entry(short).or_default() += 1;
    }

    #[must_use]
    pub fn tally(&self, s: Stage) -> Tally {
        self.stages.get(&s).cloned().unwrap_or_default()
    }

    /// A plain table, one row per stage in run order.
    #[must_use]
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut o = format!(
            "package {}\n{:<14} {:>5} {:>8} {:>8}  first reason\n",
            self.package, "stage", "ok", "not-impl", "blocked"
        );
        for s in Stage::ALL {
            let t = self.tally(s);
            let reason: String = t
                .first_reason
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
            let _ = writeln!(
                o,
                "{:<14} {:>5} {:>8} {:>8}  {reason}",
                s.name(),
                t.ok,
                t.not_implemented,
                t.blocked
            );
        }
        o
    }
}

/// What one run did: tasks, cache hits and misses per entry kind, and the
/// units it recomputed. Stage times come from the caller's clock.
#[derive(Default, Debug, Clone)]
pub struct Counters {
    pub tasks: BTreeMap<&'static str, usize>,
    pub hits: BTreeMap<&'static str, usize>,
    pub misses: BTreeMap<&'static str, usize>,
    pub modules_checked: Vec<String>,
    pub ifaces_built: Vec<String>,
    pub parsed: Vec<String>,
    /// Modules whose TIR was decoded from their `check` entry.
    pub tir_decoded: Vec<String>,
    pub emitted: usize,
    pub stage_ns: BTreeMap<&'static str, u64>,
    pub deep_hashes: BTreeMap<String, Hash128>,
    /// Per folder: the hash of its interface blob bytes.
    pub iface_blobs: BTreeMap<String, Hash128>,
    pub check_keys: BTreeMap<String, Hash128>,
    /// Per stage: the `graph` parts (`Coherence`, `InitOrder`) this run
    /// computed instead of reading their entries.
    pub parts_computed: BTreeMap<&'static str, usize>,
}

impl Counters {
    #[must_use]
    pub fn hit(&self, kind: &str) -> usize {
        self.hits.get(kind).copied().unwrap_or(0)
    }
    #[must_use]
    pub fn miss(&self, kind: &str) -> usize {
        self.misses.get(kind).copied().unwrap_or(0)
    }
    #[must_use]
    pub fn ran(&self, task: &str) -> usize {
        self.tasks.get(task).copied().unwrap_or(0)
    }
    #[must_use]
    pub fn computed(&self, stage: &str) -> usize {
        self.parts_computed.get(stage).copied().unwrap_or(0)
    }
    /// Sorts the unit lists, so a parallel run reports like a serial one.
    pub(crate) fn sort(&mut self) {
        self.modules_checked.sort();
        self.ifaces_built.sort();
        self.parsed.sort();
        self.tir_decoded.sort();
    }
}
