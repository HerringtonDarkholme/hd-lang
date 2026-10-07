//! The test runner's case model (engines-and-test-runner.md §19.1 to
//! §19.5; scheduler.md §6.5 item 4): cases in content order, `it_each`
//! row expansion, filters, and the release cursor that streams results in
//! content order.

use std::collections::BTreeMap;

/// Case kinds (§19.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CaseKind {
    It,
    ItEach,
    ItProp,
    DocTest,
}

/// Which program a case belongs to: unit tests per package, or an
/// integration test program (§19.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProgramKey {
    Unit { package: String },
    Integration { path: String },
}

/// A case's content-order key: program, then registration order, then row.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaseKey {
    pub program: ProgramKey,
    pub registration: u32,
    pub row: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub key: CaseKey,
    pub name: String,
    pub kind: CaseKind,
    pub module: String,
    pub export: u32,
}

/// A case's result (§19.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaseResult {
    Passed { ms: u64 },
    Failed { message: String },
    Panicked { category: String },
    TimedOut,
    Skipped,
}

/// The test plan: cases in content order, after `--filter` (§19.2).
#[derive(Clone, Debug, Default)]
pub struct TestPlan {
    pub cases: Vec<Case>,
}

impl TestPlan {
    #[must_use]
    pub fn new(mut cases: Vec<Case>, filter: Option<&str>) -> Self {
        if let Some(f) = filter {
            cases.retain(|c| {
                c.name.contains(f) || f.split_once('[').is_some_and(|(n, _)| c.name == n)
            });
        }
        cases.sort_by(|a, b| a.key.cmp(&b.key));
        Self { cases }
    }

    /// `it_each`: row 0 reported a count, so schedule rows 1..count.
    pub fn expand_rows(&mut self, case: &CaseKey, count: u32) {
        let Some(base) = self.cases.iter().find(|c| &c.key == case).cloned() else {
            return;
        };
        for row in 1..count {
            let mut c = base.clone();
            c.key.row = row;
            c.name = format!("{}[{row}]", base.name);
            self.cases.push(c);
        }
        self.cases.sort_by(|a, b| a.key.cmp(&b.key));
    }
}

/// Streams results in content order: a case prints once it and every
/// earlier case have finished (§6.5 item 4).
#[derive(Debug, Default)]
pub struct ReleaseCursor {
    order: Vec<CaseKey>,
    done: BTreeMap<CaseKey, CaseResult>,
    next: usize,
}

impl ReleaseCursor {
    #[must_use]
    pub fn new(plan: &TestPlan) -> Self {
        Self {
            order: plan.cases.iter().map(|c| c.key.clone()).collect(),
            done: BTreeMap::new(),
            next: 0,
        }
    }
    /// Records a finished case; returns the results now releasable, in order.
    pub fn finish(&mut self, key: CaseKey, r: CaseResult) -> Vec<(CaseKey, CaseResult)> {
        self.done.insert(key, r);
        let mut out = Vec::new();
        while let Some(k) = self.order.get(self.next) {
            let Some(r) = self.done.remove(k) else { break };
            out.push((k.clone(), r));
            self.next += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Case, CaseKey, CaseKind, CaseResult, ProgramKey, ReleaseCursor, TestPlan};

    fn case(reg: u32, name: &str) -> Case {
        Case {
            key: CaseKey {
                program: ProgramKey::Unit {
                    package: "p".into(),
                },
                registration: reg,
                row: 0,
            },
            name: name.into(),
            kind: CaseKind::It,
            module: "p.m".into(),
            export: reg,
        }
    }

    #[test]
    fn release_cursor_streams_in_content_order() {
        let plan = TestPlan::new(vec![case(2, "b"), case(1, "a"), case(3, "c")], None);
        let mut cur = ReleaseCursor::new(&plan);
        let k = |i: usize| plan.cases[i].key.clone();
        assert!(cur.finish(k(1), CaseResult::Passed { ms: 1 }).is_empty());
        let out = cur.finish(k(0), CaseResult::Passed { ms: 1 });
        assert_eq!(
            out.iter().map(|(k, _)| k.registration).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(cur.finish(k(2), CaseResult::Skipped).len(), 1);
    }

    #[test]
    fn rows_expand_and_filters_select() {
        let mut each = case(1, "rows");
        each.kind = CaseKind::ItEach;
        let mut plan = TestPlan::new(vec![each, case(2, "other")], Some("rows"));
        assert_eq!(plan.cases.len(), 1);
        let key = plan.cases[0].key.clone();
        plan.expand_rows(&key, 3);
        assert_eq!(
            plan.cases
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["rows", "rows[1]", "rows[2]"]
        );
    }
}
