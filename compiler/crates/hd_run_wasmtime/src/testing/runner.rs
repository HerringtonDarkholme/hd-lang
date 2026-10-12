use super::case::{BodyResult, CaseRun, TestCase};
use super::state::{BodyState, State};
use crate::property::{self, PlanKind};
use crate::testing_files::save_regression;

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
