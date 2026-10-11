//! Test-provider state and orchestration, separate from Wasmtime stores.
//! A property retains one successful instance; failing/discarded bodies
//! poison it. Node currently creates a fresh instance for every body.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::codec::Enc;
use crate::property::{self, Plan, PlanKind, Property};
use crate::testing_files::{Files, load_regression, save_regression};

#[derive(Clone, Debug, Default)]
pub struct TestCase {
    pub test: u32,
    pub init: u32,
    pub kind: String,
    pub temp_dir: Option<PathBuf>,
    pub seed: Option<i64>,
    pub snapshot: Option<Snapshot>,
    pub regression: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub dir: PathBuf,
    pub shown: Option<String>,
    pub slug: String,
    pub update: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicFrame {
    pub file: String,
    pub line: u32,
    pub function: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanicInfo {
    pub category: String,
    pub message: String,
    pub frames: Vec<PanicFrame>,
}

#[derive(Clone, Debug, Default)]
pub struct BodyResult {
    pub status: i64,
    pub trapped: bool,
    pub ended: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    pub panic: Option<PanicInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseRun {
    pub test: u32,
    pub status: i64,
    pub trapped: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    pub row: Option<(u32, u32)>,
    pub input: Option<String>,
    pub panic: Option<PanicInfo>,
}

impl CaseRun {
    #[must_use]
    pub fn last(&self) -> bool {
        self.row.is_none_or(|(row, rows)| row + 1 >= rows)
    }

    fn from_body(test: u32, body: BodyResult) -> Self {
        Self {
            test,
            status: body.status,
            trapped: body.trapped,
            stdout: body.stdout,
            stderr: body.stderr,
            us: body.us,
            row: None,
            input: None,
            panic: body.panic,
        }
    }
}

#[derive(Clone, Default)]
struct BodyState {
    draws: Vec<i64>,
    shown: Option<String>,
    plan: Option<Plan>,
}

struct Inner {
    case: TestCase,
    row: u32,
    rows: Option<u32>,
    property: Option<Property>,
    body: BodyState,
    timeout: Option<i64>,
    ended: bool,
    files: Files,
}

/// Shared by replacement stores, but exclusively run by one property
/// worker. A store owns a clone, not a second property controller.
#[derive(Clone)]
pub struct State(Arc<Mutex<Inner>>);

impl State {
    #[must_use]
    pub fn new(case: TestCase) -> Self {
        let files = Files::new(&case);
        Self(Arc::new(Mutex::new(Inner {
            case,
            row: 0,
            rows: None,
            property: None,
            body: BodyState::default(),
            timeout: None,
            ended: false,
            files,
        })))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn begin_body(&self, row: u32, ordinal: u64) {
        let mut state = self.lock();
        state.row = row;
        state.body = BodyState::default();
        state.timeout = None;
        state.ended = false;
        let row = state.rows.map(|_| row);
        state.files.begin(ordinal, row);
    }

    #[must_use]
    pub fn row(&self, count: u32) -> u32 {
        let mut state = self.lock();
        if state.rows.is_none() && state.property.is_none() && state.case.kind == "it_each" {
            state.rows = Some(count);
            let row = state.row;
            state.files.set_row(row);
        }
        state.row
    }

    pub fn report_timeout(&self, millis: i64) {
        self.lock().timeout = Some(millis);
    }
    #[must_use]
    pub fn current_row(&self) -> u32 {
        self.lock().row
    }
    #[must_use]
    pub fn timeout_ms(&self) -> Option<i64> {
        self.lock().timeout
    }
    #[must_use]
    pub fn ended(&self) -> bool {
        self.lock().ended
    }
    pub fn snapshot_check(&self, actual: &str) -> Result<String, String> {
        self.lock().files.snapshot_check(actual)
    }
    pub(crate) fn set_io_deadline(&self, deadline: Option<std::time::Instant>) {
        self.lock().files.deadline = deadline;
    }
    pub fn temp_dir(&self) -> Result<String, String> {
        self.lock().files.temp_dir()
    }
    pub fn cleanup_body(&self) {
        self.lock().files.cleanup();
    }

    pub fn start(&self, cases: u32, shrink: u32, examples: u32) -> Result<Option<Enc>, String> {
        let mut state = self.lock();
        if state.property.is_none() {
            let replays = load_regression(state.case.regression.as_deref(), state.files.deadline)?;
            state.property = Some(Property::new(
                cases,
                shrink,
                examples,
                state.case.seed.unwrap_or(0),
                replays,
            ));
        }
        let plan = state
            .property
            .as_mut()
            .expect("property initialized")
            .next()?;
        state.ended = plan.is_none();
        let encoded = plan.as_ref().map(Plan::encode);
        state.body.plan = plan;
        Ok(encoded)
    }

    pub fn record(&self, value: i64) {
        self.lock().body.draws.push(value);
    }
    pub fn show(&self, text: &str) {
        let mut state = self.lock();
        if state.body.shown.is_none() {
            state.body.shown = Some(text.into());
        }
    }
}

/// Native adapters retain the store only when `reuse` is true and its
/// previous body succeeded. Fresh bodies run their module init export.
pub trait BodyRunner {
    fn run_body(&mut self, state: State, reuse: bool) -> Result<BodyResult, String>;

    /// Release any retained instance when the whole test case ends,
    /// including early exits caused by provider/file/adapter errors.
    fn finish(&mut self) {}
}

#[derive(Clone)]
struct Attempt {
    draws: BodyState,
    result: BodyResult,
}

impl Attempt {
    fn failed(&self) -> bool {
        !self.result.ended && (self.result.trapped || self.result.status != 0) && !self.discarded()
    }
    fn discarded(&self) -> bool {
        // Interim fallback: the compiler does not yet export structured
        // panic globals/tables. Only the last actual panic report and its
        // exact explicit-panic message count, never a substring or stdout.
        let discard = self.result.panic.as_ref().map_or_else(
            || {
                self.result
                    .stderr
                    .lines()
                    .rev()
                    .find(|line| line.starts_with("panic: "))
                    .is_some_and(|line| {
                        line == "panic: explicit-panic: std.testing: case discarded"
                    })
            },
            |panic| {
                panic.category == "explicit-panic" && panic.message == "std.testing: case discarded"
            },
        );
        self.result.trapped && self.draws.shown.is_none() && discard
    }
    fn reusable(&self) -> bool {
        !self.result.trapped && !self.result.ended && self.result.status == 0
    }
}

fn body(
    state: &State,
    runner: &mut impl BodyRunner,
    row: u32,
    ordinal: u64,
    reuse: bool,
) -> Result<Attempt, String> {
    state.begin_body(row, ordinal);
    let result = runner.run_body(state.clone(), reuse);
    let draws = state.lock().body.clone();
    state.cleanup_body();
    Ok(Attempt {
        draws,
        result: result?,
    })
}

/// Runs ordinary cases, all table rows, or one sequential property.
/// Timing is reported but is deliberately not part of cross-engine parity.
pub fn execute_case(
    case: &TestCase,
    runner: &mut impl BodyRunner,
    emit: &mut impl FnMut(CaseRun),
) -> Result<(), String> {
    let result = execute_case_inner(case, runner, emit);
    runner.finish();
    result
}

fn execute_case_inner(
    case: &TestCase,
    runner: &mut impl BodyRunner,
    emit: &mut impl FnMut(CaseRun),
) -> Result<(), String> {
    let state = State::new(case.clone());
    let first = body(&state, runner, 0, 0, false)?;
    let rows = state.lock().rows;
    if let Some(rows) = rows {
        if rows == 0 {
            emit(CaseRun {
                test: case.test,
                row: Some((0, 0)),
                ..CaseRun::default()
            });
            return Ok(());
        }
        let mut result = CaseRun::from_body(case.test, first.result);
        result.row = Some((0, rows));
        emit(result);
        for row in 1..rows {
            let next = body(&state, runner, row, u64::from(row), false)?;
            let mut result = CaseRun::from_body(case.test, next.result);
            result.row = Some((row, rows));
            emit(result);
        }
        return Ok(());
    }
    if state.lock().property.is_some() {
        execute_property(case, &state, runner, first, emit)
    } else {
        emit(CaseRun::from_body(case.test, first.result));
        Ok(())
    }
}

fn execute_property(
    case: &TestCase,
    state: &State,
    runner: &mut impl BodyRunner,
    first: Attempt,
    emit: &mut impl FnMut(CaseRun),
) -> Result<(), String> {
    let first_result = first.result.clone();
    let mut total = first.result.us;
    let mut ordinal = 1;
    let mut current = first;
    loop {
        if current.result.ended {
            break;
        }
        if current.failed() {
            let plan = current.draws.plan.clone();
            let mut failing = current;
            if let Some(plan) = plan.filter(|plan| plan.kind != PlanKind::Example) {
                let budget = state.lock().property.as_ref().expect("property").shrink;
                let mut reuse = false;
                failing = property::shrink(
                    failing,
                    budget,
                    |attempt| &attempt.draws.draws,
                    |stream| {
                        state
                            .lock()
                            .property
                            .as_mut()
                            .expect("property")
                            .replay(stream.to_vec(), plan.size);
                        let next = body(state, runner, 0, ordinal, reuse)?;
                        ordinal += 1;
                        total = total.saturating_add(next.result.us);
                        reuse = next.reusable();
                        Ok(if next.draws.plan.is_none() {
                            property::Trial::Unavailable
                        } else if next.failed() {
                            property::Trial::Candidate(next)
                        } else {
                            property::Trial::NotFailed
                        })
                    },
                )?;
                if let Some(path) = &case.regression {
                    save_regression(path, &failing.draws.draws)?;
                }
            }
            failing.result.us = total;
            let mut result = CaseRun::from_body(case.test, failing.result);
            result.input = Some(failing.draws.shown.unwrap_or_default());
            emit(result);
            return Ok(());
        }
        let reuse = current.reusable();
        {
            let mut state = state.lock();
            let property = state.property.as_mut().expect("property");
            if current.discarded() {
                property.discarded += 1;
                if property.discarded > 10 * u64::from(property.cases) {
                    emit(CaseRun {
                        test: case.test,
                        status: 1,
                        stderr: format!(
                            "the property discarded {} cases, more than 10 times its {} cases\n",
                            property.discarded, property.cases
                        ),
                        us: total,
                        ..CaseRun::default()
                    });
                    return Ok(());
                }
            } else if let Some(plan) = &current.draws.plan {
                property.pass(plan.kind);
            }
            if !property.more() {
                break;
            }
        }
        current = body(state, runner, 0, ordinal, reuse)?;
        ordinal += 1;
        total = total.saturating_add(current.result.us);
    }
    let mut result = CaseRun::from_body(case.test, first_result);
    result.status = 0;
    result.trapped = false;
    result.panic = None;
    result.us = total;
    emit(result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BodyResult, BodyRunner, CaseRun, PanicInfo, State, TestCase, execute_case};
    use crate::codec::{Dec, Enc};

    struct Fake<F>(F);
    impl<F: FnMut(State, bool) -> Result<BodyResult, String>> BodyRunner for Fake<F> {
        fn run_body(&mut self, state: State, reuse: bool) -> Result<BodyResult, String> {
            (self.0)(state, reuse)
        }
    }

    fn decode_plan(encoded: &Enc) -> (i64, i64) {
        let mut decoder = Dec::new(encoded.done());
        if decoder.leb().expect("example flag") != 0 {
            decoder.leb().expect("example");
        }
        (decoder.zz().expect("seed"), decoder.zz().expect("size"))
    }

    #[test]
    fn successful_property_bodies_reuse_one_instance_in_seed_order() {
        let mut reuse_flags = Vec::new();
        let mut plans = Vec::new();
        let mut fake = Fake(|state: State, reuse: bool| {
            reuse_flags.push(reuse);
            let encoded = state.start(3, 0, 0)?.expect("fresh plan");
            plans.push(decode_plan(&encoded));
            state.show("input");
            Ok(BodyResult {
                us: 7,
                ..BodyResult::default()
            })
        });
        let case = TestCase {
            seed: Some(10),
            ..TestCase::default()
        };
        let mut results = Vec::new();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
        assert_eq!(reuse_flags, [false, true, true]);
        assert_eq!(plans, [(10, 0), (11, 1), (12, 2)]);
        assert_eq!(results.len(), 1);
        assert_eq!(
            (results[0].status, results[0].us, results[0].input.as_ref()),
            (0, 21, None)
        );
    }

    fn discarded() -> BodyResult {
        BodyResult {
            status: 3,
            trapped: true,
            stderr: "panic: explicit-panic: std.testing: case discarded\n".into(),
            panic: Some(PanicInfo {
                category: "explicit-panic".into(),
                message: "std.testing: case discarded".into(),
                ..PanicInfo::default()
            }),
            ..BodyResult::default()
        }
    }

    #[test]
    fn discard_replaces_instance_but_same_panic_after_show_is_failure() {
        let mut calls = Vec::new();
        let mut fake = Fake(|state: State, reuse: bool| {
            calls.push(reuse);
            state.start(1, 0, 0)?.expect("plan");
            if calls.len() == 1 {
                Ok(discarded())
            } else {
                state.show("shown");
                Ok(BodyResult::default())
            }
        });
        let case = TestCase::default();
        let mut results = Vec::new();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
        assert_eq!(calls, [false, false]);
        assert_eq!(results[0].status, 0);
        let mut fake = Fake(|state: State, _: bool| {
            state.start(1, 0, 0)?.expect("plan");
            state.show("bad input");
            Ok(discarded())
        });
        results.clear();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
        assert_eq!(results[0].input.as_deref(), Some("bad input"));
        assert!(results[0].trapped);
    }

    #[test]
    fn discard_limit_is_exact_and_never_reuses_a_poisoned_instance() {
        let mut calls = 0;
        let mut fake = Fake(|state: State, reuse: bool| {
            assert!(!reuse);
            calls += 1;
            state.start(2, 0, 0)?.expect("plan");
            let mut result = discarded();
            result.panic = None; // Approved interim Node-output fallback.
            Ok(result)
        });
        let mut results = Vec::new();
        execute_case(&TestCase::default(), &mut fake, &mut |result| {
            results.push(result);
        })
        .expect("run");
        assert_eq!(calls, 21);
        assert_eq!(results[0].status, 1);
        assert_eq!(
            results[0].stderr,
            "the property discarded 21 cases, more than 10 times its 2 cases\n"
        );
    }

    #[test]
    fn table_rows_are_fresh_and_empty_tables_have_no_result_fields() {
        for count in [0, 3] {
            let mut rows = Vec::new();
            let mut fake = Fake(|state: State, reuse: bool| {
                assert!(!reuse);
                rows.push(state.row(count));
                Ok(BodyResult::default())
            });
            let case = TestCase {
                test: 9,
                kind: "it_each".into(),
                ..TestCase::default()
            };
            let mut results = Vec::new();
            execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
            if count == 0 {
                assert_eq!(rows, [0]);
                assert_eq!(
                    results,
                    [CaseRun {
                        test: 9,
                        row: Some((0, 0)),
                        ..CaseRun::default()
                    }]
                );
            } else {
                assert_eq!(rows, [0, 1, 2]);
                assert_eq!(
                    results.iter().map(|r| r.row).collect::<Vec<_>>(),
                    [Some((0, 3)), Some((1, 3)), Some((2, 3))]
                );
            }
            assert!(results.last().expect("result").last());
        }
    }

    #[test]
    fn body_error_still_removes_lazy_temporary_directory() {
        let mut path = None;
        let mut fake = Fake(|state: State, _: bool| {
            let directory = std::path::PathBuf::from(state.temp_dir()?);
            assert!(directory.is_dir());
            path = Some(directory);
            Err("adapter failed".into())
        });
        assert!(execute_case(&TestCase::default(), &mut fake, &mut |_| {}).is_err());
        assert!(!path.expect("lazy directory").exists());
    }

    #[test]
    fn property_exhaustion_is_success_without_a_guest_panic() {
        let mut fake = Fake(|state: State, _: bool| {
            assert!(state.start(0, 0, 0)?.is_none());
            assert!(state.ended());
            Ok(BodyResult {
                ended: true,
                ..BodyResult::default()
            })
        });
        let mut results = Vec::new();
        execute_case(&TestCase::default(), &mut fake, &mut |result| {
            results.push(result);
        })
        .expect("run");
        assert_eq!(results[0].status, 0);
        assert!(!results[0].trapped);
    }

    #[test]
    fn retained_adapter_is_finalized_on_both_success_and_error() {
        struct Retained {
            fail: bool,
            finished: bool,
        }
        impl BodyRunner for Retained {
            fn run_body(&mut self, _: State, _: bool) -> Result<BodyResult, String> {
                if self.fail {
                    Err("failed".into())
                } else {
                    Ok(BodyResult::default())
                }
            }
            fn finish(&mut self) {
                self.finished = true;
            }
        }
        for fail in [false, true] {
            let mut runner = Retained {
                fail,
                finished: false,
            };
            assert_eq!(
                execute_case(&TestCase::default(), &mut runner, &mut |_| {}).is_err(),
                fail
            );
            assert!(runner.finished);
        }
    }

    #[test]
    fn shrinks_save_recorded_draws_and_only_passing_attempts_reuse_instances() {
        let owner = State::new(TestCase::default());
        let root = std::path::PathBuf::from(owner.temp_dir().expect("regression root"));
        let path = root.join("regressions/one");
        let mut reuse_flags = Vec::new();
        let mut fake = Fake(|state: State, reuse: bool| {
            reuse_flags.push(reuse);
            let encoded = state.start(1, 6, 0)?.expect("plan");
            let mut decoder = Dec::new(encoded.done());
            assert_eq!(decoder.leb().expect("no example"), 0);
            decoder.zz().expect("seed");
            decoder.zz().expect("size");
            let count = decoder.leb().expect("replay length");
            let draw = if count == 0 {
                8
            } else {
                decoder.zz().expect("replay draw")
            };
            state.record(draw);
            state.show(&draw.to_string());
            Ok(BodyResult {
                status: if draw >= 3 { 3 } else { 0 },
                trapped: draw >= 3,
                us: 1,
                ..BodyResult::default()
            })
        });
        let case = TestCase {
            regression: Some(path.clone()),
            ..TestCase::default()
        };
        let mut results = Vec::new();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("shrink run");
        assert_eq!(reuse_flags, [false, false, false, true, false, true, false]);
        assert_eq!(std::fs::read_to_string(&path).expect("saved"), "3\n");
        assert_eq!(results[0].input.as_deref(), Some("3"));
        assert_eq!(results[0].us, 7);
        assert!(results[0].trapped);
        let mut replayed = Vec::new();
        let mut fake = Fake(|state: State, _: bool| {
            let encoded = state
                .start(0, 0, 0)?
                .expect("saved plan even without fresh cases");
            let mut decoder = Dec::new(encoded.done());
            assert_eq!(decoder.leb().expect("no example"), 0);
            decoder.zz().expect("seed");
            decoder.zz().expect("size");
            assert_eq!(decoder.leb().expect("one draw"), 1);
            replayed.push(decoder.zz().expect("saved draw"));
            state.show("saved");
            Ok(BodyResult::default())
        });
        execute_case(&case, &mut fake, &mut |_| {}).expect("replay");
        assert_eq!(replayed, [3]);
    }

    #[test]
    fn shrink_timeout_before_start_preserves_the_counterexample_and_stops() {
        let owner = State::new(TestCase::default());
        let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
        let path = root.join("regression");
        let mut calls = 0;
        let mut fake = Fake(|state: State, _: bool| {
            calls += 1;
            if calls == 1 {
                state.start(1, 1000, 0)?.expect("initial plan");
                state.record(8);
                state.show("counterexample");
            }
            if calls == 2 {
                // Match the actual native adapter: the replay's CLI deadline
                // has expired before PropertyRunner.start can run.
                state.set_io_deadline(Some(std::time::Instant::now()));
            }
            Ok(BodyResult {
                status: 3,
                trapped: true,
                stderr: if calls == 1 {
                    "panic: explicit-panic: assertion failed\n".into()
                } else {
                    "panic: time-limit: the program exceeded its time limit\n".into()
                },
                ..BodyResult::default()
            })
        });
        let case = TestCase {
            regression: Some(path.clone()),
            ..TestCase::default()
        };
        let mut results = Vec::new();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).unwrap();
        assert_eq!(
            calls, 2,
            "an unavailable replay stops the whole shrink loop"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "8\n");
        assert_eq!(results[0].input.as_deref(), Some("counterexample"));
        assert_eq!(
            results[0].stderr,
            "panic: explicit-panic: assertion failed\n"
        );
        let mut later = Fake(|_: State, _: bool| Ok(BodyResult::default()));
        execute_case(
            &TestCase {
                test: 1,
                ..TestCase::default()
            },
            &mut later,
            &mut |result| results.push(result),
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].test, 1);
        assert_eq!(results[1].status, 0);
    }
}
